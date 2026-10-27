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
 * 默认折叠。**运行中也能折叠** —— 用户需要把正在跑的活动收起来腾出空间
 * （之前 running 时强制展开，`toggle` 直接 return，用户点不动）。
 *
 * 运行中首次出现时自动展开一次，让用户看到实时输出；之后以用户选择为准
 * （`userTouched` 一旦为真就不再自动展开）。
 */
const userExpanded = ref(false)
const userTouched = ref(false)
const expanded = computed(() => userExpanded.value)

// 从「未运行」进入「运行中」时自动展开一次（仅当用户没手动操作过）
watch(running, (nowRunning, wasRunning) => {
  if (nowRunning && !wasRunning && !userTouched.value) userExpanded.value = true
})

/**
 * 摘要行右侧细节（`· {detailText}`）。
 *
 * - 仍在生成 → 不显示（spinner 已表达「正在运行」，再加文字就重复了）
 * - 已结束   → `用时 X`
 * - 缺时间戳 → 不显示
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
  // 运行中不显示用时：数字会一直定格在首条消息的时刻，看起来像卡住了
  if (running.value) return null
  const { first, last } = span.value
  if (first === null || last === null || last < first) return null
  return `用时 ${formatDuration(last - first)}`
})

/**
 * 正在执行的工具名（运行中时作为标题，替换掉笼统的「活动」）。
 *
 * 取**最后一条仍在跑**的工具调用，按 `kind` 映射成动词短语：
 * `execute` → 「正在运行命令」、`edit` → 「正在编辑文件」…
 *
 * 没有正在跑的工具（比如模型只是在思考）时退回「正在处理」。
 */
const KIND_RUNNING_LABEL: Record<string, string> = {
  execute: '正在运行命令',
  edit: '正在编辑文件',
  delete: '正在删除文件',
  move: '正在移动文件',
  read: '正在读取文件',
  search: '正在搜索',
  fetch: '正在抓取网页',
  think: '正在思考',
}

/** 从工具标题推断动词短语（老消息没有 kind 时的兜底） */
function runningLabelByName(name: string): string {
  const t = name.toLowerCase()
  if (/(运行|执行|命令|command|bash|shell|terminal|exec)/.test(t)) return '正在运行命令'
  if (/(编辑|修改|写入|write|edit|patch|apply)/.test(t)) return '正在编辑文件'
  if (/(读取|已读|查看|read|cat|open)/.test(t)) return '正在读取文件'
  if (/(搜索|查找|grep|search|find|glob)/.test(t)) return '正在搜索'
  return '正在执行'
}

const runningToolLabel = computed(() => {
  if (!running.value) return null
  for (let i = props.messages.length - 1; i >= 0; i -= 1) {
    const tools = props.messages[i].toolCalls ?? []
    for (let j = tools.length - 1; j >= 0; j -= 1) {
      const tc = tools[j]
      if (tc.status !== 'running') continue
      if (tc.kind && KIND_RUNNING_LABEL[tc.kind]) return KIND_RUNNING_LABEL[tc.kind]
      return runningLabelByName(tc.name)
    }
  }
  return '正在处理'
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
  userTouched.value = true
  userExpanded.value = !userExpanded.value
}


</script>

<template>
  <div class="mb-3">
    <!--
      摘要行：状态图标 + 「活动 · 用时 X」+ 折叠箭头。

      状态图标紧贴文字（不靠 flex-1 推到右端）—— 之前 spinner 跑到最右侧，
      和文字脱节、看起来像两个不相干的元素。
      运行中时不再额外加「正在运行」文字，避免与 spinner 重复表达同一件事。
    -->
    <button
      class="group flex w-full cursor-pointer items-center gap-1.5 rounded-md px-1.5 py-1 text-xs transition-colors hover:bg-neutral-100"
      :class="running ? 'cursor-default text-blue-600' : 'text-neutral-600 hover:text-neutral-800'"
      :title="running ? '正在运行' : expanded ? '收起' : '展开'"
      @click="toggle"
    >
      <!-- 状态位：运行中转圈，结束显示折叠箭头 -->
      <LoaderCircle v-if="running" class="h-3.5 w-3.5 shrink-0 animate-spin text-blue-500" />
      <ChevronDown
        v-else-if="expanded"
        class="h-3.5 w-3.5 shrink-0 text-neutral-400"
      />
      <ChevronRight v-else class="h-3.5 w-3.5 shrink-0 text-neutral-400" />

      <ListTree class="h-3.5 w-3.5 shrink-0 text-neutral-400" />
      <!-- 运行中显示具体在做什么（正在运行命令…），结束后回到「活动 · 用时」 -->
      <span v-if="running && runningToolLabel" class="running-label">{{
        runningToolLabel
      }}</span>
      <span v-else>活动</span>
      <span v-if="detailText" class="text-neutral-400">· {{ detailText }}</span>
    </button>

    <!--
      展开内容：按原顺序渲染这一段。
      `-mb-4` 抵消末尾 MessageItem 自带的间距，避免与下一段隔得太开。
    -->
    <Transition name="expand">
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
    </Transition>
  </div>
</template>

<style scoped>
/*
 * 运行中标题的扫光动画。
 *
 * 用 `background-clip: text` 做渐变扫光（与全局 `.shimmer` 同思路，但这里
 * 需要更慢、更弱的节奏 —— 它常驻在对话流里，太跳会干扰阅读）。
 * `prefers-reduced-motion` 下退化为纯色，不做动画。
 */
.running-label {
  background-image: linear-gradient(
    90deg,
    #2563eb 0%,
    #2563eb 35%,
    #93c5fd 50%,
    #2563eb 65%,
    #2563eb 100%
  );
  background-size: 250% 100%;
  background-repeat: no-repeat;
  -webkit-background-clip: text;
  background-clip: text;
  color: transparent;
  animation: running-sweep 2.4s linear infinite;
}

@keyframes running-sweep {
  from {
    background-position: 100% center;
  }
  to {
    background-position: 0% center;
  }
}

@media (prefers-reduced-motion: reduce) {
  .running-label {
    animation: none;
    color: #2563eb;
    -webkit-text-fill-color: currentColor;
  }
}

/* 折叠内容的展开动画：高度渐变，避免整块突然出现 */
.expand-enter-active,
.expand-leave-active {
  transition:
    opacity 0.16s ease,
    transform 0.16s ease;
}
.expand-enter-from,
.expand-leave-to {
  opacity: 0;
  transform: translateY(-2px);
}
</style>
