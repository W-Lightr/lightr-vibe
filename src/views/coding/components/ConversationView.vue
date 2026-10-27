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
 * 把消息按「用户提问 → 该轮 assistant 输出」切成段。
 *
 * 每组 = 一条用户消息 + 紧随其后、turnId 相同的 assistant 消息（思考/工具/正文）。
 * 没有 turnId 的旧消息各自成组，保持原样平铺 —— 不做猜测式合并。
 */
type Block =
  | { kind: 'user'; message: Session['messages'][number] }
  | { kind: 'turn'; turnId: string; messages: Session['messages']; generating: boolean }

const blocks = computed<Block[]>(() => {
  const out: Block[] = []
  const msgs = props.session.messages
  let i = 0
  while (i < msgs.length) {
    const m = msgs[i]
    if (m.role === 'user') {
      out.push({ kind: 'user', message: m })
      i += 1
      continue
    }
    // assistant 侧：收集同一 turnId 的连续消息
    const turnId = m.turnId
    const group = [m]
    i += 1
    if (turnId) {
      while (i < msgs.length && msgs[i].role !== 'user' && msgs[i].turnId === turnId) {
        group.push(msgs[i])
        i += 1
      }
    }
    out.push({
      kind: 'turn',
      turnId: turnId ?? `solo-${m.id}`,
      messages: group,
      // 只有「最后一段」且整体仍在生成时才算运行中，避免历史轮次永远转圈
      generating: generating.value && i >= msgs.length,
    })
  }
  return out
})
</script>

<template>
  <div class="relative min-h-0 flex-1">
    <div ref="listRef" class="no-scrollbar h-full overflow-y-auto" @scroll="onScroll">
      <div class="mx-auto max-w-3xl px-6 py-6">
        <template v-for="block in blocks" :key="block.kind === 'user' ? block.message.id : block.turnId">
          <MessageItem v-if="block.kind === 'user'" :message="block.message" />
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
