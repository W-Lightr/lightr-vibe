<script setup lang="ts">
// 一轮「中间过程」的收纳单元。
//
// 用户规则：
// 1. 连续的工具调用要折叠（不逐条平铺）；
// 2. 有文字就显示文字；
// 3. 除最后一轮外，所有轮次的结果都收纳。
//
// 摘要行固定是「活动 · 用时 X」（运行中为「活动 · 正在运行」），不显示工具条数。
//
// 展开后**连续的过程消息仍会再折叠一层**（除非中间夹着正文），避免一轮里
// 几十条工具一次性平铺。
//
// 折叠内不显示「重新生成 / 从此分叉」—— 那两个动作只挂在最后一条输出上。
import { computed, ref, watch } from 'vue'
import { ChevronDown, ChevronRight, ListTree, LoaderCircle } from 'lucide-vue-next'
import type { Message } from '../../../types/session'
import { formatDuration } from '../../../utils/time'
import MessageItem from './MessageItem.vue'

const props = defineProps<{
  /** 这一段的过程消息（按原顺序） */
  messages: Message[]
  /** 这一段是否仍在生成 */
  generating: boolean
  /** 摘要行左侧的文字提示（默认按内容推导） */
  label?: string | null
}>()


/** 仍有消息在流式输出 */
const streaming = computed(() =>
  props.messages.some((m) => m.role === 'assistant' && m.status === 'streaming'),
)

const running = computed(() => props.generating || streaming.value)

/**
 * 展开态。
 *
 * 默认折叠（历史轮次全部收起）；但正在生成的这一段强制展开，否则用户看不到
 * 实时输出。用派生值而非 watch 改 state —— 否则用户手动折叠会被下一帧撑开。
 */
const userExpanded = ref(false)
const expanded = computed(() => running.value || userExpanded.value)

/**
 * 摘要行文字：`活动 · 用时 X`（用户指定用「活动」+「用时」，不再显示工具条数）。
 *
 * - 仍在生成 → `活动 · 正在运行`
 * - 已结束   → `活动 · 用时 X`
 * - 缺时间戳 → 只有 `活动`
 */
/** 本段首尾时刻（用于「用时」） */
const span = computed(() => {
  let first: number | null = null
  let last: number | null = null
  for (const m of props.messages) {
    const t = m.createdAt
    if (typeof t !== 'number') continue
    if (first === null || t < first) first = t
    if (last === null || t > last) last = t
  }
  return { first, last }
})

const elapsedText = computed(() => {
  if (running.value) return '正在运行'
  const { first, last } = span.value
  if (first === null || last === null || last < first) return null
  return `用时 ${formatDuration(last - first)}`
})

/** 摘要右侧的细节（用时 / 自定义 label），没有就不显示 */
const detailText = computed(() => props.label ?? elapsedText.value)

/**
 * 展开后的渲染序列：把**连续的**过程性消息合成一个子分组，正文各自独立。
 *
 * 用户规则：活动中折叠的内容里，连续工具调用也要折叠，除非中间有输出内容。
 * 所以展开 `ActivityGroup` 不应把几十条工具平铺出来 —— 连续过程仍收成一个小节。
 */
type SubItem =
  | { kind: 'single'; key: string; message: Message }
  | { kind: 'chunk'; key: string; messages: Message[] }

function isProcess(m: Message): boolean {
  if (m.role !== 'assistant') return false
  return m.type === 'thinking' || (m.toolCalls?.length ?? 0) > 0
}

const subItems = computed<SubItem[]>(() => {
  const out: SubItem[] = []
  let buffer: Message[] = []
  const flush = () => {
    if (buffer.length === 0) return
    out.push({ kind: 'chunk', key: `chunk-${buffer[0].id}`, messages: buffer })
    buffer = []
  }
  for (const m of props.messages) {
    if (isProcess(m)) {
      buffer.push(m)
      continue
    }
    flush()
    out.push({ kind: 'single', key: m.id, message: m })
  }
  flush()
  return out
})

/** 子分组各自的展开态（默认折叠；只在用户点击时打开） */
const openChunks = ref<Record<string, boolean>>({})

function chunkSummary(msgs: Message[]): string {
  const tools = msgs.reduce((n, m) => n + (m.toolCalls?.length ?? 0), 0)
  const hasThinking = msgs.some((m) => m.type === 'thinking')
  if (tools > 0) return hasThinking ? `思考与 ${tools} 个工具` : `${tools} 个工具`
  return '思考'
}

function toggleChunk(key: string) {
  openChunks.value = { ...openChunks.value, [key]: !openChunks.value[key] }
}

function toggle() {
  if (running.value) return
  userExpanded.value = !userExpanded.value
}

// 生成结束那一刻把展开权交还用户，保持当前视觉不跳
watch(running, (nowRunning, wasRunning) => {
  if (wasRunning && !nowRunning) userExpanded.value = true
})
</script>

<template>
  <div class="mb-3">
    <!--
      摘要行（codex 风格）：方框图标 + 「活动」+ 用时，右侧折叠箭头。
      状态用前置图标表达（转圈 / 完成），不用额外文字。
    -->
    <button
      class="group flex w-full cursor-pointer items-center gap-1.5 rounded-md px-1.5 py-1 text-xs text-neutral-500 transition-colors hover:bg-neutral-100 hover:text-neutral-700"
      :class="running && 'cursor-default'"
      :title="running ? '正在运行' : expanded ? '收起' : '展开'"
      @click="toggle"
    >
      <ListTree class="h-3.5 w-3.5 shrink-0 text-neutral-400" />
      <span :class="running ? 'text-blue-600' : 'text-neutral-600'">活动</span>
      <span v-if="detailText" class="text-neutral-400">· {{ detailText }}</span>

      <span class="flex-1" />

      <span class="flex h-3.5 w-3.5 shrink-0 items-center justify-center">
        <LoaderCircle v-if="running" class="h-3 w-3 animate-spin text-blue-500" />
        <ChevronDown v-else-if="expanded" class="h-3.5 w-3.5 text-neutral-300" />
        <ChevronRight v-else class="h-3.5 w-3.5 text-neutral-300" />
      </span>
    </button>

    <!--
      展开内容：按原顺序渲染这一段。
      `-mb-4` 抵消末尾 MessageItem 自带的间距，避免与下一段隔得太开。
    -->
    <div v-if="expanded" class="-mb-4 mt-1 border-l-2 border-neutral-200/70 pl-3">
      <template v-for="item in subItems" :key="item.key">
        <!-- 正文：直接显示 -->
        <MessageItem
          v-if="item.kind === 'single'"
          :message="item.message"
          :show-actions="false"
        />
        <!-- 连续过程：仍折叠成一个小节，避免几十条工具平铺 -->
        <div v-else class="mb-2">
          <button
            class="flex w-full cursor-pointer items-center gap-1.5 rounded-md px-1.5 py-1 text-xs text-neutral-500 transition-colors hover:bg-neutral-100 hover:text-neutral-700"
            @click="toggleChunk(item.key)"
          >
            <ChevronDown
              v-if="openChunks[item.key]"
              class="h-3.5 w-3.5 shrink-0 text-neutral-400"
            />
            <ChevronRight v-else class="h-3.5 w-3.5 shrink-0 text-neutral-400" />
            <span>{{ chunkSummary(item.messages) }}</span>
          </button>
          <div
            v-if="openChunks[item.key]"
            class="-mb-4 mt-1 border-l-2 border-neutral-200/70 pl-3"
          >
            <MessageItem
              v-for="m in item.messages"
              :key="m.id"
              :message="m"
              :show-actions="false"
            />
          </div>
        </div>
      </template>
    </div>
  </div>
</template>
