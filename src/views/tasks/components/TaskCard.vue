<script setup lang="ts">
// 任务卡片：状态 chip + 标题（两行截断）+ 项目名 + 相对时间 + 状态动作按钮。
// 只读任务行，不碰 store / service——所有动作 emit 给 TasksView 决定实现（子任务 3 接会话）。
// 卡片本体承担拖拽 pointerdown；按钮区 @pointerdown.stop 避免动作按钮误触发拖拽。
import { computed } from 'vue'
import { Check, CornerDownLeft, LoaderCircle, Play, Square } from 'lucide-vue-next'
import { formatRelativeTime } from '../../../utils/time'
import { TASK_STATUS_LABELS } from '../../../types/task'
import type { WorkTask } from '../../../types/task'

const props = defineProps<{
  task: WorkTask
  /** 项目名（全部项目视图显示；单项目视图可不传） */
  projectName?: string
  /** 任务所属项目已从项目列表移除（prd PAC4） */
  projectMissing: boolean
  /** 是否允许拖拽（全部项目视图 = false） */
  dragEnabled: boolean
  /** 正在被拖拽（原卡片降至 opacity-40） */
  dragging: boolean
  /** 抖动中（非法落列回弹提示） */
  shaking: boolean
  /** 该任务有一次转移在途（按钮置灰，防双击产生 stale） */
  busy: boolean
  /** 主按钮文案（null = 无主按钮，见 PRD R4 状态矩阵） */
  primaryLabel: string | null
  /** 次按钮文案（null = 无次按钮） */
  secondaryLabel: string | null
}>()

const emit = defineEmits<{
  /** 卡片根元素（供拖拽注册，卸载时传 null） */
  element: [el: HTMLElement | null]
  'pointer-down': [e: PointerEvent]
  /** 点击卡片本体（非按钮）→ 打开详情 */
  open: []
  primary: []
  secondary: []
}>()

type ChipKind = 'running' | 'attention' | 'done' | 'failed' | 'neutral'

/** chip 视觉分组（穷举，新增状态必须归组） */
const chipKind = computed<ChipKind>(() => {
  switch (props.task.status) {
    case 'queued':
    case 'preparing':
    case 'running':
    case 'merging':
      return 'running'
    case 'awaiting_input':
    case 'review':
      return 'attention'
    case 'done':
      return 'done'
    case 'failed':
      return 'failed'
    case 'todo':
    case 'canceled':
      return 'neutral'
    default: {
      const unhandled: never = props.task.status
      return unhandled
    }
  }
})

const CHIP_CLASS: Record<ChipKind, string> = {
  running: 'border-blue-200 bg-blue-50 text-blue-600',
  attention: 'border-amber-200 bg-amber-50 text-amber-700',
  done: 'border-green-200 bg-green-50 text-green-700',
  failed: 'border-red-200 bg-red-50 text-red-600',
  neutral: 'border-neutral-200 bg-neutral-100 text-neutral-500',
}

/** 运行中 / 队列中显示旋转图标 */
const spinning = computed(() => chipKind.value === 'running')

/** 失败时优先展示归因文案（被中断单独提示） */
const chipLabel = computed(() => {
  if (props.task.status === 'failed' && props.task.failureReason === 'interrupted') return '已中断'
  return TASK_STATUS_LABELS[props.task.status]
})

/** 主按钮语义分组：决定图标（开始/停止/通过） */
const primaryIcon = computed<'start' | 'stop' | 'approve' | null>(() => {
  switch (props.task.status) {
    case 'todo':
    case 'failed':
      return 'start'
    case 'queued':
    case 'preparing':
    case 'running':
    case 'awaiting_input':
      return 'stop'
    case 'review':
      return 'approve'
    default:
      return null
  }
})

/** ref 回调：把卡片根 DOM 交给父级拖拽注册表 */
function setRef(el: unknown): void {
  emit('element', el instanceof HTMLElement ? el : null)
}
</script>

<template>
  <article
    :ref="setRef"
    class="group relative cursor-grab overflow-hidden rounded-md border border-neutral-200 bg-white transition-colors hover:border-neutral-300 active:cursor-grabbing"
    :class="[!dragEnabled && 'cursor-default', dragging && 'opacity-40', shaking && 'animate-shake']"
    :data-task-id="task.id"
    @pointerdown="emit('pointer-down', $event)"
  >
    <!-- 顶部细色条：仅失败 / 待审核做轻提示 -->
    <div v-if="task.status === 'failed'" class="h-0.5 w-full bg-red-400" />
    <div v-else-if="task.status === 'review'" class="h-0.5 w-full bg-amber-400" />

    <div class="px-2.5 py-2" @click="emit('open')">
      <!-- 状态 chip + 相对时间 -->
      <div class="flex items-center gap-1.5">
        <span
          class="inline-flex items-center gap-1 rounded-full border px-1.5 py-px text-[10px] leading-4"
          :class="CHIP_CLASS[chipKind]"
        >
          <LoaderCircle v-if="spinning" class="h-2.5 w-2.5 animate-spin" />
          <Check v-else-if="chipKind === 'done'" class="h-2.5 w-2.5" />
          {{ chipLabel }}
        </span>
        <span class="flex-1" />
        <span class="shrink-0 text-[10px] text-neutral-400">
          {{ formatRelativeTime(task.updatedAt) }}
        </span>
      </div>

      <!-- 标题：最多两行 -->
      <p class="mt-1.5 line-clamp-2 text-[13px] leading-[18px] text-neutral-800" :title="task.title">
        {{ task.title }}
      </p>

      <!-- 项目名 / 项目已移除提示 -->
      <div v-if="projectName || projectMissing" class="mt-1 flex min-w-0 items-center gap-1">
        <span
          v-if="projectMissing"
          class="shrink-0 rounded bg-neutral-100 px-1 py-px text-[10px] text-neutral-400"
        >
          项目已移除
        </span>
        <span v-else class="truncate text-[11px] text-neutral-500">{{ projectName }}</span>
      </div>
    </div>

    <!-- 动作区：每状态至多一主一次（PRD R4 矩阵）；阻止冒泡避免触发拖拽/打开详情 -->
    <div
      v-if="primaryLabel || secondaryLabel"
      class="flex items-center gap-1.5 px-2.5 pb-2"
      @pointerdown.stop
      @click.stop
    >
      <button
        v-if="primaryLabel"
        class="flex h-6 cursor-pointer items-center gap-1 rounded-md px-2 text-[11px] transition-colors disabled:cursor-not-allowed disabled:opacity-40"
        :class="
          primaryIcon === 'approve'
            ? 'bg-blue-500 text-white hover:bg-blue-600'
            : 'border border-neutral-200 bg-white text-neutral-600 hover:bg-neutral-50'
        "
        :disabled="busy"
        @click="emit('primary')"
      >
        <LoaderCircle v-if="busy" class="h-3 w-3 animate-spin" />
        <Play v-else-if="primaryIcon === 'start'" class="h-3 w-3" />
        <Square v-else-if="primaryIcon === 'stop'" class="h-2.5 w-2.5" />
        <Check v-else-if="primaryIcon === 'approve'" class="h-3 w-3" />
        {{ primaryLabel }}
      </button>
      <button
        v-if="secondaryLabel"
        class="flex h-6 cursor-pointer items-center gap-1 rounded-md border border-neutral-200 bg-white px-2 text-[11px] text-neutral-600 transition-colors hover:bg-neutral-50 disabled:cursor-not-allowed disabled:opacity-40"
        :disabled="busy"
        @click="emit('secondary')"
      >
        <CornerDownLeft class="h-3 w-3" />
        {{ secondaryLabel }}
      </button>
    </div>
  </article>
</template>

<style scoped>
/* 非法落列回弹：横向抖动一次（design §5：「卡片抖动一次」） */
.animate-shake {
  animation: task-shake 0.4s cubic-bezier(0.36, 0.07, 0.19, 0.97) both;
}
@keyframes task-shake {
  10%,
  90% {
    transform: translateX(-2px);
  }
  20%,
  80% {
    transform: translateX(3px);
  }
  30%,
  50%,
  70% {
    transform: translateX(-4px);
  }
  40%,
  60% {
    transform: translateX(4px);
  }
}
@media (prefers-reduced-motion: reduce) {
  .animate-shake {
    animation: none;
  }
}
</style>
