<script setup lang="ts">
// 连续「思考 + 工具调用」的收纳单元。
//
// 只承载**过程性内容**（thinking 消息 / 带 toolCalls 的 assistant 消息）。
// AI 的正文回答永远由 ConversationView 直接渲染，绝不进这里 —— 正文是用户要看
// 的结果，不是过程噪音。
//
// 为什么需要：多轮工具调用时，之前每个消息各自平铺一行「思考完毕」和「N 个
// 工具」，一屏几十行、内容高度重复。
//
// 摘要文案：
// - 本轮仍在生成 → 「正在运行」+ 转圈
// - 已结束       → 「用时 1 分钟 34 秒」（无时间戳时退回「活动」）
//
// 默认折叠；生成中的轮次默认**展开**，否则用户看不到实时输出。
import { computed, ref, watch } from 'vue'
import { ChevronDown, ChevronRight, LoaderCircle } from 'lucide-vue-next'
import type { Message } from '../../../types/session'
import MessageItem from './MessageItem.vue'
import { formatDuration } from '../../../utils/time'

const props = defineProps<{
  /** 本轮的全部消息（按原顺序，含 thinking / tool / text / error） */
  messages: Message[]
  /** 本轮是否仍在生成 */
  generating: boolean
}>()

/** 本轮最后一条 assistant 消息（用于判断是否收敛） */
const allStreaming = computed(() =>
  props.messages.some((m) => m.role === 'assistant' && m.status === 'streaming'),
)

/** 仍有内容在流式输出，或本轮整体生成中 */
const running = computed(() => props.generating || allStreaming.value)

/**
 * 是否展开。
 *
 * 默认折叠；生成中强制展开（用户需要看到实时输出），结束后回到用户自己的
 * 选择。这里用「生成中视为展开」的派生值，而不是在 watch 里改 state ——
 * 否则用户手动折叠会被下一帧的 watch 又撑开。
 */
const userExpanded = ref(false)
const expanded = computed(() => running.value || userExpanded.value)

/** 本轮开始时刻：取第一条消息的 createdAt（毫秒） */
const startedAt = computed(() => {
  const first = props.messages[0]
  return first?.createdAt ?? null
})

/**
 * 结束时刻。
 *
 * `Message` 只有 `createdAt`（消息创建时刻，没有更新时间），所以用**最后一条
 * 消息的 createdAt** 作为本轮结束的近似 —— 对「对话轮次」够用，因为正文消息
 * 通常在流式开始时就创建了。不做假的时钟推进。
 */
const endedAt = computed(() => {
  if (running.value) return null
  let latest: number | null = null
  for (const m of props.messages) {
    const t = m.createdAt
    if (typeof t === 'number' && (latest === null || t > latest)) latest = t
  }
  return latest
})

/**
 * 摘要文案。
 *
 * 运行中 → 「正在运行」；已结束且有起止时间 → 「用时 X」；
 * 缺时间戳 → 「活动」（不编造假的时长）。
 */
const summary = computed(() => {
  if (running.value) return '正在运行'
  const begin = startedAt.value
  const end = endedAt.value
  if (begin !== null && end !== null && end >= begin) {
    return `用时 ${formatDuration(end - begin)}`
  }
  return '活动'
})

/** 本轮的工具调用总数（折叠时也能一眼看出规模） */
const toolCount = computed(() =>
  props.messages.reduce((sum, m) => sum + (m.toolCalls?.length ?? 0), 0),
)

function toggle() {
  if (running.value) return // 生成中不可折叠
  userExpanded.value = !userExpanded.value
}

// 一轮结束的瞬间把展开状态交还给用户（保持当前视觉不跳，初始视为已展开）
watch(running, (nowRunning, wasRunning) => {
  if (wasRunning && !nowRunning) userExpanded.value = true
})
</script>

<template>
  <div class="mb-4">
    <!-- 摘要行：正在运行 / 用时 X，点击展开或收起 -->
    <button
      class="group flex h-7 w-full cursor-pointer items-center gap-1.5 rounded-md px-1.5 text-xs text-neutral-500 transition-colors hover:bg-neutral-100 hover:text-neutral-700"
      :class="running && 'cursor-default'"
      :title="running ? '正在运行' : expanded ? '收起本轮' : '展开本轮'"
      @click="toggle"
    >
      <LoaderCircle v-if="running" class="h-3.5 w-3.5 shrink-0 animate-spin text-blue-500" />
      <ChevronDown v-else-if="expanded" class="h-3.5 w-3.5 shrink-0 text-neutral-400" />
      <ChevronRight v-else class="h-3.5 w-3.5 shrink-0 text-neutral-400" />

      <span :class="running && 'text-blue-600'">{{ summary }}</span>

      <!-- 折叠时补充规模信息，避免「用时 X」过空 -->
      <span v-if="!expanded && toolCount > 0" class="text-neutral-400">
        · {{ toolCount }} 个工具
      </span>
    </button>

    <!--
      展开内容：本轮全部消息按原顺序渲染。

      `-mb-3` 抵消最后一条 MessageItem 自带的 `mb-5`，否则组内末尾会多出
      一大段空白，跟下一个回合隔得太开。
    -->
    <div v-if="expanded" class="-mb-3 mt-1 border-l-2 border-neutral-200/70 pl-3 pb-1">
      <MessageItem v-for="m in messages" :key="m.id" :message="m" />
    </div>
  </div>
</template>
