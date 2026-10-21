<script setup lang="ts">
// 任务卡片：状态 chip + 标题（两行截断）+ 项目名 + 相对时间 + 状态动作按钮。
// chip 视觉/文案来自 taskChip.ts（与详情抽屉同一来源）；动作全部 emit 给 TasksView，
// 由它调 taskSessionBridge（启动链路 / 停止 / 审核）——卡片只表达意图。
// 卡片本体承担拖拽 pointerdown；按钮区 @pointerdown.stop 避免动作按钮误触发拖拽。
import { computed } from 'vue'
import { Check, CornerDownLeft, LoaderCircle, Play, Square } from 'lucide-vue-next'
import { formatRelativeTime } from '../../../utils/time'
import TaskStatusChip from './TaskStatusChip.vue'
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
    class="group relative cursor-grab overflow-hidden rounded-lg border border-transparent bg-white transition-all hover:border-neutral-200 hover:shadow-[0_1px_2px_rgba(0,0,0,0.04)] active:cursor-grabbing"
    :class="[!dragEnabled && 'cursor-default', dragging && 'opacity-40', shaking && 'animate-shake']"
    :data-task-id="task.id"
    @pointerdown="emit('pointer-down', $event)"
  >
    <div class="px-3 py-2.5" @click="emit('open')">
      <!-- 标题：最多两行，codex 风格以标题为主视觉 -->
      <p class="line-clamp-2 text-[13px] leading-[18px] font-medium text-neutral-800" :title="task.title">
        {{ task.title }}
      </p>

      <!-- 元信息行：状态 chip + 项目名 + 相对时间 -->
      <div class="mt-2 flex min-w-0 items-center gap-1.5">
        <TaskStatusChip :task="task" />
        <span
          v-if="projectMissing"
          class="shrink-0 rounded bg-neutral-100 px-1 py-px text-[10px] text-neutral-400"
        >
          项目已移除
        </span>
        <span v-else-if="projectName" class="min-w-0 truncate text-[11px] text-neutral-500">
          {{ projectName }}
        </span>
        <span class="flex-1" />
        <span class="shrink-0 text-[10px] tabular-nums text-neutral-400">
          {{ formatRelativeTime(task.updatedAt) }}
        </span>
      </div>
    </div>

    <!-- 动作区：默认隐藏，悬停/聚焦时显现（codex 的克制感），避免卡片常驻重按钮 -->
    <div
      v-if="primaryLabel || secondaryLabel"
      class="flex items-center gap-1.5 px-3 pb-2.5 opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100"
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
