<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ArrowDown, MessageSquarePlus } from 'lucide-vue-next'
import type { Session } from '../../../types/session'
import MessageItem from './MessageItem.vue'
import TurnGroup from './TurnGroup.vue'
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
 * 关键规则（用户要求）：**AI 的正文回答永远可见，不进折叠**。
 * 只有「连续的思考 / 工具调用」这种过程性内容才收进 `TurnGroup`。
 *
 * 切法（按消息原顺序走一遍）：
 * - user                → 单独渲染
 * - thinking / 带工具的 assistant → 攒进缓冲，遇到正文或用户消息时收束成一个 activity
 * - 纯正文 assistant    → 直接渲染（不折叠），并把前面攒的 activity 先冲出来
 * - error               → 直接渲染
 */
type Block =
  | { kind: 'single'; message: Session['messages'][number] }
  | { kind: 'activity'; key: string; messages: Session['messages']; generating: boolean }

/** 过程性消息：思考块，或带工具调用的 assistant 消息 */
function isActivityMessage(m: Session['messages'][number]): boolean {
  if (m.role !== 'assistant') return false
  return m.type === 'thinking' || (m.toolCalls?.length ?? 0) > 0
}

const blocks = computed<Block[]>(() => {
  const out: Block[] = []
  const msgs = props.session.messages
  let buffer: Session['messages'] = []

  const flush = (isLastChunk: boolean) => {
    if (buffer.length === 0) return
    const last = buffer[buffer.length - 1]
    out.push({
      kind: 'activity',
      key: `act-${last.id}`,
      messages: buffer,
      // 只有「位于消息末尾」的那段过程内容才算仍在运行，历史轮次不会转圈
      generating: isLastChunk && generating.value,
    })
    buffer = []
  }

  for (let i = 0; i < msgs.length; i += 1) {
    const m = msgs[i]
    if (isActivityMessage(m)) {
      buffer.push(m)
      continue
    }
    // 正文 / 错误 / 用户消息：先把前面攒的过程内容冲出去
    flush(false)
    out.push({ kind: 'single', message: m })
  }
  // 收尾：末尾还攒着过程内容，说明本轮仍在跑
  flush(true)

  return out
})</script>

<template>
  <div class="relative min-h-0 flex-1">
    <div ref="listRef" class="no-scrollbar h-full overflow-y-auto" @scroll="onScroll">
      <div class="mx-auto max-w-3xl px-6 py-6">
        <template v-for="block in blocks" :key="block.kind === 'single' ? block.message.id : block.key">
          <MessageItem v-if="block.kind === 'single'" :message="block.message" />
          <TurnGroup v-else :messages="block.messages" :generating="block.generating" />
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
        <div v-if="generating" class="mb-4 flex items-center gap-2 text-xs text-neutral-400">
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
