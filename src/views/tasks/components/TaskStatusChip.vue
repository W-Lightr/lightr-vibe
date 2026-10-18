<script setup lang="ts">
// 任务状态 chip（纯展示）：卡片与详情抽屉共用，配色/文案的唯一来源是 taskChip.ts。
import { computed } from 'vue'
import { Check, LoaderCircle } from 'lucide-vue-next'
import { TASK_CHIP_CLASS, taskChipKind, taskChipLabel } from './taskChip'
import type { WorkTask } from '../../../types/task'

const props = defineProps<{
  task: WorkTask
  /** 尺寸：卡片默认；详情头部用 md */
  size?: 'sm' | 'md'
}>()

const kind = computed(() => taskChipKind(props.task))
const label = computed(() => taskChipLabel(props.task))
const running = computed(() => kind.value === 'running')
const sizeClass = computed(() => (props.size === 'md' ? 'px-2 py-0.5 text-[11px]' : 'px-1.5 py-px text-[10px]'))
</script>

<template>
  <span
    class="inline-flex items-center gap-1 rounded-full border leading-4"
    :class="[TASK_CHIP_CLASS[kind], sizeClass]"
  >
    <LoaderCircle v-if="running" class="h-2.5 w-2.5 animate-spin" />
    <Check v-else-if="kind === 'done'" class="h-2.5 w-2.5" />
    {{ label }}
  </span>
</template>
