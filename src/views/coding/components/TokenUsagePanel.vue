<script setup lang="ts">
import { computed } from 'vue'
import type { TokenUsage } from '../../../types/session'
import { formatDuration, formatTokens } from '../../../utils/time'

const props = defineProps<{ usage: TokenUsage; durationMs: number }>()

const percent = computed(() =>
  props.usage.contextWindow > 0
    ? Math.min(100, (props.usage.total / props.usage.contextWindow) * 100)
    : 0,
)

const hasData = computed(() => props.usage.total > 0)

const barColor = computed(() => {
  if (percent.value > 85) return 'bg-red-400'
  if (percent.value > 60) return 'bg-amber-400'
  return 'bg-blue-400'
})
</script>

<template>
  <div>
    <div class="grid grid-cols-3 gap-2">
      <div class="rounded-md border border-neutral-200 bg-neutral-50/60 px-2.5 py-2">
        <div class="text-[11px] text-neutral-400">Token 总量</div>
        <div class="mt-0.5 text-sm font-medium text-neutral-800">{{ formatTokens(usage.total) }}</div>
      </div>
      <div class="rounded-md border border-neutral-200 bg-neutral-50/60 px-2.5 py-2">
        <div class="text-[11px] text-neutral-400">输入</div>
        <div class="mt-0.5 text-sm font-medium text-neutral-800">{{ formatTokens(usage.input) }}</div>
      </div>
      <div class="rounded-md border border-neutral-200 bg-neutral-50/60 px-2.5 py-2">
        <div class="text-[11px] text-neutral-400">输出</div>
        <div class="mt-0.5 text-sm font-medium text-neutral-800">{{ formatTokens(usage.output) }}</div>
      </div>
    </div>

    <div class="mt-3">
      <div class="flex items-center justify-between text-[11px]">
        <span class="text-neutral-400">上下文窗口</span>
        <span class="text-neutral-600">
          {{ formatTokens(usage.total) }} / {{ formatTokens(usage.contextWindow) }}
          <span class="ml-1 font-medium" :class="percent > 85 ? 'text-red-500' : 'text-neutral-700'">
            {{ percent.toFixed(1) }}%
          </span>
        </span>
      </div>
      <div class="mt-1.5 h-1.5 overflow-hidden rounded-full bg-neutral-200">
        <div
          class="h-full rounded-full transition-all"
          :class="barColor"
          :style="{ width: `${hasData ? Math.max(percent, 1) : 0}%` }"
        />
      </div>
      <p v-if="!hasData" class="mt-1.5 text-[11px] text-neutral-400">暂无用量数据</p>
    </div>

    <div class="mt-3 flex items-center justify-between text-[11px]">
      <span class="text-neutral-400">本次会话耗时</span>
      <span class="text-neutral-600">{{ formatDuration(durationMs) }}</span>
    </div>
  </div>
</template>
