<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ArrowDown, MessageSquarePlus } from 'lucide-vue-next'
import type { Session } from '../../../types/session'
import MessageItem from './MessageItem.vue'
import ActivityGroup from './ActivityGroup.vue'
import PlanCard from './PlanCard.vue'
import { useConversationScroll } from '../../../composables/useConversationScroll'
import { useSessionStore } from '../../../stores/session'
import { useWorkspaceStore } from '../../../stores/workspace'
import { isTauri } from '../../../utils/isTauri'

const props = defineProps<{ session: Session }>()

const workspace = useWorkspaceStore()
const sessionStore = useSessionStore()
const listRef = ref<HTMLElement | null>(null)

const { onScroll, restore, followIfAtBottom, scrollToBottom, showBackToLatest } =
  useConversationScroll(listRef)

// 打开会话标签时懒加载落盘消息（design §4）
watch(
  () => props.session.id,
  (id) => {
    void sessionStore.ensureMessages(id)
    void restore(id)
  },
  { immediate: true },
)

// 消息新增或流式增量：处于底部才跟随
watch(
  () => {
    const ms = props.session.messages
    const last = ms[ms.length - 1]
    return `${ms.length}:${last?.content.length ?? 0}:${last?.toolCalls?.length ?? 0}`
  },
  () => void followIfAtBottom(),
)

const generating = computed(() => workspace.isGenerating(props.session.id))

/**
 * 渲染单元。
 *
 * 用户规则：
 * 1. 连续的工具调用要折叠（不逐条平铺）；
 * 2. 有文字就显示文字；
 * 3. **除最后一轮外，所有轮次的结果都收纳**。
 *
 * 所以：
 * - 历史轮次 → 整段收成一个 `ActivityGroup`（一条摘要行）
 * - 最后一轮 → 正文直接渲染，只把过程性消息（思考 / 带工具）折叠
 */
type Block =
  | { kind: 'single'; key: string; message: Session['messages'][number] }
  | {
      kind: 'activity'
      key: string
      messages: Session['messages']
      generating: boolean
      label: string | null
    }

/** 过程性消息：思考块，或带工具调用的 assistant 消息 */
function isProcess(m: Session['messages'][number]): boolean {
  if (m.role !== 'assistant') return false
  return m.type === 'thinking' || (m.toolCalls?.length ?? 0) > 0
}

/** 是否存在「正在生成」的活动块（用于抑制重复的底部提示） */
const hasLiveActivity = computed(() =>
  blocks.value.some((b) => b.kind === 'activity' && b.generating),
)

const blocks = computed<Block[]>(() => {
  const msgs = props.session.messages

  // 按用户消息切段：每段 = 一条用户消息之后的一轮 assistant 产出
  const segments: Session['messages'][] = []
  let current: Session['messages'] = []
  const userSlots: { message: Session['messages'][number]; segIndex: number }[] = []

  for (const m of msgs) {
    if (m.role === 'user') {
      if (current.length > 0) segments.push(current)
      current = []
      userSlots.push({ message: m, segIndex: segments.length })
      continue
    }
    current.push(m)
  }
  if (current.length > 0) segments.push(current)

  const lastSeg = segments.length - 1
  const out: Block[] = []

  // 开头若有 assistant 消息（无前导用户消息），单独成段
  const hasLeadingAssistant = msgs.length > 0 && msgs[0].role !== 'user'
  let segCursor = 0

  if (hasLeadingAssistant && segments.length > 0) {
    const seg = segments[0]
    pushSegment(seg, segCursor === lastSeg)
    segCursor = 1
  }

  for (const slot of userSlots) {
    out.push({ kind: 'single', key: slot.message.id, message: slot.message })
    const seg = segments[segCursor]
    if (seg) {
      pushSegment(seg, segCursor === lastSeg)
      segCursor += 1
    }
  }

  /**
   * 把一段过程变成渲染块（可能产出多个块）。
   *
   * - **历史轮次**：整段收进一个 ActivityGroup（一条摘要行）。
   * - **最后一轮**：正文消息仍然直接渲染，只把「过程性消息」（思考 / 带工具）
   *   收进 ActivityGroup。这与「AI 输出内容不要收纳」保持一致 —— 最后一轮是
   *   用户当下要看的结果，不能藏。
   */
  function pushSegment(seg: Session['messages'], isLast: boolean): void {
    if (!isLast) {
      out.push({
        kind: 'activity',
        key: `act-${seg[0].id}`,
        messages: seg,
        generating: false,
        label: null,
      })
      return
    }

    // 最后一轮：过程性消息折叠，正文直接平铺。
    //
    // **最后一条「正文」消息必须走 `single`** —— 它是「复制 / 重新生成 /
    // 从此分叉」工具栏的宿主。若把它收进 ActivityGroup（那边 show-actions=false），
    // 工具栏就整个消失了。
    //
    // 注意：工具消息也是 `type: 'text'` 但 content 为空，所以不能用「最后一条
    // 消息」判断，必须找「最后一条有正文内容的」。
    let lastBodyIdx = -1
    for (let i = seg.length - 1; i >= 0; i -= 1) {
      const m = seg[i]
      if (m.role === 'assistant' && m.type === 'text' && m.content.trim() !== '') {
        lastBodyIdx = i
        break
      }
    }
    // 整轮没有任何正文（纯工具）：让最后一条消息单独渲染，工具栏才不会消失。
    // MessageItem 内部对空正文有自己的兜底渲染。
    if (lastBodyIdx === -1 && seg.length > 0) {
      lastBodyIdx = seg.length - 1
    }

    let buffer: Session['messages'] = []
    const flush = (atEnd: boolean) => {
      if (buffer.length === 0) return
      out.push({
        kind: 'activity',
        key: `act-${buffer[0].id}`,
        messages: buffer,
        generating: atEnd && generating.value,
        label: null,
      })
      buffer = []
    }

    for (let i = 0; i < seg.length; i += 1) {
      const m = seg[i]
      // 承载操作栏的那条正文单独渲染；其余过程消息照常折叠
      if (isProcess(m) && i !== lastBodyIdx) {
        buffer.push(m)
        continue
      }
      flush(false)
      out.push({ kind: 'single', key: m.id, message: m })
    }
    flush(true)
  }
  return out
})
</script>

<template>
  <div class="relative min-h-0 flex-1">
    <div ref="listRef" class="no-scrollbar h-full overflow-y-auto" @scroll="onScroll">
      <div class="mx-auto max-w-3xl px-6 py-6">
        <template v-for="block in blocks" :key="block.key">
          <MessageItem v-if="block.kind === 'single'" :message="block.message" />
          <ActivityGroup
            v-else
            :messages="block.messages"
            :generating="block.generating"
            :label="block.label"
          />
        </template>

        <!-- 空会话引导 -->
        <div v-if="session.messages.length === 0" class="flex flex-col items-center gap-3 pt-28 text-neutral-400">
          <MessageSquarePlus class="h-8 w-8 text-neutral-300" />
          <p class="text-sm">开始一段新的对话</p>
          <p class="text-xs">
            {{ isTauri() ? '由 OpenCode Agent 提供真实回复' : '当前为本地 Mock 服务，回复内容仅用于界面演示' }}
          </p>
        </div>

        <!-- 计划卡（plan update）：随会话 planEntries 渲染，无条目则隐藏 -->
        <PlanCard :entries="session.planEntries ?? []" />

        <!-- 生成中提示：脉冲蓝点 + 微光扫过文字（Shimmer） -->
        <div
          v-if="generating && !hasLiveActivity"
          class="mb-4 flex items-center gap-2 text-xs text-neutral-400"
        >
          <span class="h-1.5 w-1.5 animate-pulse rounded-full bg-blue-500" />
          <span class="shimmer font-medium">正在生成…</span>
        </div>
      </div>
    </div>

    <!-- 返回最新消息 -->
    <button
      v-if="showBackToLatest"
      class="absolute right-6 bottom-4 flex h-8 w-8 cursor-pointer items-center justify-center rounded-full border border-neutral-200 bg-white text-neutral-500 shadow-sm hover:bg-neutral-50"
      title="回到最新消息"
      @click="scrollToBottom"
    >
      <ArrowDown class="h-4 w-4" />
    </button>
  </div>
</template>
