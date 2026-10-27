<script setup lang="ts">
// 一轮「中间过程」的收纳单元。
//
// 用户的规则：
// 1. 有文字就显示文字；
// 2. 文字**后面**的连续工具调用要折叠起来（不逐条平铺）；
// 3. **除最后一轮外，所有轮次的结果都收纳**。
//
// 所以这里收纳的是「一段过程」：可能是几次文字 + 工具交错，收成一条摘要，
// 展开后按原顺序看到文字与工具分组。
//
// 摘要行的文案优先级：
// - 仍在生成 → 「正在运行」+ 转圈
// - 有工具 → 「{N} 个工具」
// - 只有思考/文字 → 「活动」
import { computed, ref, watch } from 'vue'
import { ChevronDown, ChevronRight, LoaderCircle } from 'lucide-vue-next'
import type { Message } from '../../../types/session'
import MessageItem from './MessageItem.vue'

const props = defineProps<{
  /** 这一段的过程消息（按原顺序） */
  messages: Message[]
  /** 这一段是否仍在生成 */
  generating: boolean
  /** 摘要行左侧的文字提示（默认按内容推导） */
  label?: string | null
}>()

const toolCount = computed(() =>
  props.messages.reduce((sum, m) => sum + (m.toolCalls?.length ?? 0), 0),
)

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
 * 摘要行文字。
 *
 * 优先级（用户要求「有文字显示文字」）：
 * 1. 仍在生成 → 「正在运行」
 * 2. 本段有正文 → 取**首条正文**的开头（截断），让折叠行也能看出这轮在讲什么
 * 3. 只有工具 → 「N 个工具」
 * 4. 其他 → 「活动」
 */
const firstText = computed(() => {
  for (const m of props.messages) {
    if (m.role !== 'assistant' || m.type !== 'text') continue
    const t = m.content.replace(/\s+/g, ' ').trim()
    if (t) return t
  }
  return null
})

const summary = computed(() => {
  if (running.value) return '正在运行'
  if (props.label) return props.label
  if (firstText.value) {
    const t = firstText.value
    return t.length > 40 ? `${t.slice(0, 40)}…` : t
  }
  if (toolCount.value > 0) return `${toolCount.value} 个工具`
  return '活动'
})

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
    <!-- 摘要行 -->
    <button
      class="flex h-7 w-full cursor-pointer items-center gap-1.5 rounded-md px-1.5 text-xs text-neutral-500 transition-colors hover:bg-neutral-100 hover:text-neutral-700"
      :class="running && 'cursor-default'"
      :title="running ? '正在运行' : expanded ? '收起' : '展开'"
      @click="toggle"
    >
      <LoaderCircle v-if="running" class="h-3.5 w-3.5 shrink-0 animate-spin text-blue-500" />
      <ChevronDown v-else-if="expanded" class="h-3.5 w-3.5 shrink-0 text-neutral-400" />
      <ChevronRight v-else class="h-3.5 w-3.5 shrink-0 text-neutral-400" />
      <span :class="running && 'text-blue-600'">{{ summary }}</span>
    </button>

    <!--
      展开内容：按原顺序渲染这一段。
      `-mb-4` 抵消末尾 MessageItem 自带的间距，避免与下一段隔得太开。
    -->
    <div v-if="expanded" class="-mb-4 mt-1 border-l-2 border-neutral-200/70 pl-3">
      <MessageItem v-for="m in messages" :key="m.id" :message="m" />
    </div>
  </div>
</template>
