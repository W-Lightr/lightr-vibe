<script setup lang="ts">
// 一轮回复底部的统计栏。
//
// 元素：用时 · token · 复制 · 重新生成 · 从此分叉 · 完成时刻。
//
// 降级规则（关键，别写成「总是显示所有字段」）：
// - token 全为 0 视为「没有数据」→ 不显示。一条真实回复不可能花 0 token，
//   显示「0 tokens」比不显示更像坏了（codeg 同判断）。
// - 用时缺失 → 不显示。
// - 重新生成 / 分叉只在最新一轮可用；历史轮置灰并说明原因（不是直接消失，
//   否则整行图标会跳动）。
import { computed, ref } from 'vue'
import { Check, Copy, GitBranch, RefreshCw } from 'lucide-vue-next'
import { formatDateTime, formatDuration, formatRelativeTime, formatTokens } from '../../../utils/time'
import type { TokenUsage } from '../../../types/session'

const props = defineProps<{
  durationMs: number | null
  usage: TokenUsage | null
  /** 要复制的文本（正文拼接）；空串时禁用复制 */
  copyText: string
  /** 是否是最新一轮（决定重新生成/分叉可用） */
  current: boolean
  /** 本轮结束时刻（毫秒）；null 表示仍在生成或未知 */
  completedAt: number | null
  /** 是否仍在生成 */
  running: boolean
}>()

const emit = defineEmits<{
  regenerate: []
  fork: []
}>()

const copied = ref(false)

/**
 * 是否有可用的 token 数据。
 *
 * `total` 为 0 或全部字段为 0 都当「没有数据」—— 与 codeg 对全 0 usage 的判断一致。
 */
const hasUsage = computed(() => {
  const u = props.usage
  if (!u) return false
  return u.total > 0 || u.input > 0 || u.output > 0
})

const durationLabel = computed(() =>
  props.durationMs === null ? null : formatDuration(props.durationMs),
)

const tokenLabel = computed(() => (hasUsage.value ? formatTokens(props.usage?.total ?? 0) : null))

/** 完成时刻：短格式显示，悬停看完整时间 */
const completedLabel = computed(() =>
  props.completedAt === null ? null : formatRelativeTime(props.completedAt),
)
const completedTitle = computed(() =>
  props.completedAt === null ? null : formatDateTime(props.completedAt),
)

const canCopy = computed(() => props.copyText.trim().length > 0)

/** 历史轮不能重新生成/分叉，用 tooltip 说明原因，而不是隐藏按钮 */
const actionDisabledReason = computed(() =>
  props.current ? null : '只有最新一轮可以重新生成或分叉',
)

async function copy() {
  if (!canCopy.value) return
  try {
    await navigator.clipboard.writeText(props.copyText)
    copied.value = true
    setTimeout(() => (copied.value = false), 1500)
  } catch {
    // 剪贴板不可用（无权限 / 非安全上下文）：静默失败，不打断阅读
  }
}

function regenerate() {
  if (!props.current) return
  emit('regenerate')
}

function fork() {
  if (!props.current) return
  emit('fork')
}

/** 按钮共用样式 */
const btnClass =
  'flex h-6 w-6 cursor-pointer items-center justify-center rounded-md text-neutral-400 transition-colors hover:bg-neutral-100 hover:text-neutral-600'
const btnDisabledClass = 'cursor-not-allowed opacity-40 hover:bg-transparent hover:text-neutral-400'
</script>

<template>
  <!--
    统计栏顺序：按钮组 → 用时 / token / 完成时刻。
    生成中不显示（数据还在变，数字会一直跳）。
  -->
  <div
    v-if="!running"
    class="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-[11px] text-neutral-400"
  >
    <!-- 操作按钮组（贴左） -->
    <button
      :class="[btnClass, !canCopy && btnDisabledClass]"
      :disabled="!canCopy"
      title="复制正文"
      @click="copy"
    >
      <Check v-if="copied" class="h-3.5 w-3.5 text-green-600" />
      <Copy v-else class="h-3.5 w-3.5" />
    </button>

    <button
      :class="[btnClass, !current && btnDisabledClass]"
      :disabled="!current"
      :title="actionDisabledReason ?? '重新生成'"
      @click="regenerate"
    >
      <RefreshCw class="h-3.5 w-3.5" />
    </button>

    <button
      :class="[btnClass, !current && btnDisabledClass]"
      :disabled="!current"
      :title="actionDisabledReason ?? '从此分叉'"
      @click="fork"
    >
      <GitBranch class="h-3.5 w-3.5" />
    </button>

    <!-- 时间与用量（在按钮右侧） -->
    <span v-if="durationLabel" class="ml-1 tabular-nums">{{ durationLabel }}</span>
    <span v-if="durationLabel && tokenLabel" class="text-neutral-300">·</span>
    <span v-if="tokenLabel" class="tabular-nums" title="本轮消耗的 token">
      {{ tokenLabel }} tokens
    </span>
    <span v-if="completedLabel" class="tabular-nums" :title="completedTitle ?? ''">
      · {{ completedLabel }}
    </span>

    <span class="flex-1" />
  </div>
</template>
