<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ArrowDown, MessageSquarePlus } from 'lucide-vue-next'
import type { Session } from '../../../types/session'
import MessageItem from './MessageItem.vue'
import AssistantTurn from './AssistantTurn.vue'
import { toContentParts } from '../../../utils/contentParts'
import type { TokenUsage } from '../../../types/session'
import PlanCard from './PlanCard.vue'
import { useConversationScroll } from '../../../composables/useConversationScroll'
import { useConversation } from '../../../composables/useConversation'
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
 * - 每轮交给 `AssistantTurn`，由它派生 parts 并切出「进度 / 尾部回答」
 * - 进度可折叠，尾部回答恒可见
 */
type Block =
  | { kind: 'user'; key: string; message: Session['messages'][number] }
  | {
      kind: 'turn'
      key: string
      /** 本轮全部 assistant 消息（按原顺序） */
      messages: Session['messages']
      /** 本轮最后一条消息 id —— 重新生成 / 分叉用它定位 */
      lastMessageId: string
      streaming: boolean
      current: boolean
      /**
       * 默认是否展开。
       *
       * 只有**正在生成**的那一轮默认展开；历史会话打开时全部折叠。
       * 之前直接用 `current`（= 最后一段）当默认值，导致打开旧会话时
       * 最后一轮自动展开，看起来像「内容全都没折叠」。
       */
      defaultOpen: boolean
      /** 会话所属项目 —— 文件卡片点开时需要它定位项目 */
      projectId: string
      durationMs: number | null
      completedAt: number | null
      usage: TokenUsage | null
    }

/**
 * 把消息序列切成「用户气泡 / assistant 轮次」的块列表。
 *
 * 一条用户消息之后、下一条用户消息之前的所有 assistant 消息算一轮。
 * 每轮交给 `AssistantTurn`，由它做 parts 派生与「进度 / 回答」切分 ——
 * 这一层只负责分组和计算用时，不做任何内容判断。
 */
const blocks = computed<Block[]>(() => {
  const msgs = props.session.messages
  const out: Block[] = []
  let turn: Session['messages'] = []

  const flushTurn = (isLast: boolean) => {
    if (turn.length === 0) return
    const first = turn[0]
    const last = turn[turn.length - 1]
    const streaming = isLast && generating.value
    // 用时 = 首尾消息 createdAt 之差。Message 没有 updatedAt，这是近似值
    // （正文消息在流式开始时就创建，实际耗时会略偏短）。
    const durationMs =
      last.createdAt >= first.createdAt ? last.createdAt - first.createdAt : null
    out.push({
      kind: 'turn',
      key: `turn-${first.id}`,
      messages: turn,
      lastMessageId: last.id,
      streaming,
      current: isLast,
      defaultOpen: streaming,
      projectId: props.session.projectId ?? '',
      durationMs,
      completedAt: streaming ? null : last.createdAt,
      usage: isLast ? (props.session.tokenUsage ?? null) : null,
    })
    turn = []
  }

  for (const m of msgs) {
    if (m.role === 'user') {
      flushTurn(false)
      out.push({ kind: 'user', key: m.id, message: m })
      continue
    }
    turn.push(m)
  }
  flushTurn(true)

  return out
})

/**
 * 折叠态，按轮次 key 存开合。
 *
 * `foldEpoch` 在每次发送新消息时 +1，旧 epoch 的记录一律失效 ——
 * 「发送新消息收起上方所有轮次」因此只是一次计数器自增，而不是遍历线程
 * （遍历会丢掉用户手动展开过的记录）。
 */
const openTurns = ref<Record<string, boolean>>({})
const turnEpoch = ref<Record<string, number>>({})
const foldEpoch = ref(0)

/** 本轮是否展开：当前轮默认展开，历史轮默认收起 */
function isTurnOpen(block: Extract<Block, { kind: 'turn' }>): boolean {
  if (turnEpoch.value[block.key] !== foldEpoch.value) return block.defaultOpen
  return openTurns.value[block.key] ?? block.defaultOpen
}

function setTurnOpen(key: string, open: boolean) {
  openTurns.value = { ...openTurns.value, [key]: open }
  turnEpoch.value = { ...turnEpoch.value, [key]: foldEpoch.value }
}

// 用户消息数变化 = 发送了新消息 → epoch +1 → 上方全部回到默认收起
watch(
  () => props.session.messages.filter((m) => m.role === 'user').length,
  () => {
    foldEpoch.value += 1
  },
)

/**
 * 最后一段（当前轮）是否仍在生成。
 *
 * 用于抑制底部的「正在生成…」兜底提示 —— 当前轮的进度行已经有 spinner，
 * 两处同时提示属于重复表达。
 */
const lastTurnStreaming = computed(() => {
  const msgs = props.session.messages
  for (let i = msgs.length - 1; i >= 0; i -= 1) {
    if (msgs[i].role === 'assistant') {
      return generating.value && i === msgs.length - 1
    }
  }
  return false
})

const { regenerate, fork } = useConversation()

function onRegenerate(block: Extract<Block, { kind: 'turn' }>) {
  regenerate(props.session.id, block.lastMessageId)
}

function onFork(block: Extract<Block, { kind: 'turn' }>) {
  const forked = fork(props.session.id, block.lastMessageId)
  if (forked) workspace.openSession(forked.id)
}
</script>

<template>
  <div class="relative min-h-0 flex-1">
    <div ref="listRef" class="no-scrollbar h-full overflow-y-auto" @scroll="onScroll">
      <div class="mx-auto max-w-3xl px-6 py-6">
        <template v-for="block in blocks" :key="block.key">
          <MessageItem v-if="block.kind === 'user'" :message="block.message" />
          <AssistantTurn
            v-else
            :parts="toContentParts(block.messages)"
            :streaming="block.streaming"
            :current="block.current"
            :duration-ms="block.durationMs"
            :usage="block.usage"
            :completed-at="block.completedAt"
            :open="isTurnOpen(block)"
            :project-id="block.projectId"
            @update:open="(open) => setTurnOpen(block.key, open)"
            @regenerate="onRegenerate(block)"
            @fork="onFork(block)"
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

        <!--
          生成中兜底提示：三个点依次跳起（typing indicator）。
          之前是「脉冲蓝点 + 扫光文字」，蓝点与下方内容抢注意力，且扫光在长文本
          状态里显得吵；三点跳动是最小、最不打扰的「还在动」信号。
        -->
        <div
          v-if="generating && !lastTurnStreaming"
          class="mb-4 flex items-center gap-1.5 text-xs text-neutral-400"
        >
          <span class="typing-dots" aria-hidden="true" title="正在生成">
            <span />
            <span />
            <span />
          </span>
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

<style scoped>
/*
 * 三点跳动加载指示。
 * 每个点用同样的 keyframes，靠 animation-delay 错开，形成依次起落的节奏。
 * 用 opacity + translateY 而不是缩放：在 12px 文字旁缩放几乎看不出来，
 * 位移才让人感到「在动」。
 */
.typing-dots {
  display: inline-flex;
  align-items: flex-end;
  gap: 3px;
  height: 16px;
}

.typing-dots > span {
  width: 6px;
  height: 6px;
  border-radius: 9999px;
  background-color: currentColor;
  animation: typing-bounce 1.2s ease-in-out infinite;
}

.typing-dots > span:nth-child(2) {
  animation-delay: 0.16s;
}

.typing-dots > span:nth-child(3) {
  animation-delay: 0.32s;
}

@keyframes typing-bounce {
  0%,
  60%,
  100% {
    transform: translateY(0);
    opacity: 0.4;
  }
  30% {
    transform: translateY(-5px);
    opacity: 1;
  }
}

@media (prefers-reduced-motion: reduce) {
  .typing-dots > span {
    animation: none;
    opacity: 0.6;
  }
}
</style>
