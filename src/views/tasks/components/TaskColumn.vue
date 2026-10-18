<script setup lang="ts">
// 看板列：标题 + 计数 + 卡片列表 + 空态；列体是滚动容器（拖拽落列按它的矩形判定）。
// 列根元素带 data-column-id，供拖拽 composable 注册；高亮只表示「合法落列」（PRD R5）。
import { computed } from 'vue'
import { Plus } from 'lucide-vue-next'
import TaskCard from './TaskCard.vue'
import { columnLabel, emptyColumnHint } from '../boardColumns'
import type { BoardColumnId } from '../boardColumns'
import type { WorkTask } from '../../../types/task'

const props = defineProps<{
  columnId: BoardColumnId
  tasks: WorkTask[]
  /** 项目筛选（null = 全部项目：卡片显示项目名、禁止拖拽） */
  allProjects: boolean
  /** 拖拽是否被全局禁用 */
  dragEnabled: boolean
  /** 正在被拖拽的任务 id */
  draggingTaskId: string | null
  /** 抖动中的任务 id */
  shakingTaskId: string | null
  /** 当前指针是否悬停在本列且落列合法 */
  dropHover: boolean
  /** 加载中（显示骨架） */
  loading: boolean
  /** 正在转移中的任务 id 集合（按钮置灰，防双击） */
  busyTaskIds: Set<string>
  /** 项目名解析：projectId → 名称（缺失返回 null） */
  projectNameOf: (projectId: string) => string | null
  /** 主 / 次按钮文案（按任务状态） */
  primaryLabelOf: (task: WorkTask) => string | null
  secondaryLabelOf: (task: WorkTask) => string | null
}>()

const emit = defineEmits<{
  'register-column': [el: HTMLElement | null]
  'register-card': [taskId: string, el: HTMLElement | null]
  'card-pointer-down': [e: PointerEvent, task: WorkTask]
  'card-open': [task: WorkTask]
  'card-primary': [task: WorkTask]
  'card-secondary': [task: WorkTask]
  /** 空态里的「新建任务」 */
  'create-task': []
}>()

const label = computed(() => columnLabel(props.columnId))
const hint = computed(() => emptyColumnHint(props.columnId))

/** 待办列空态额外给「新建任务」入口（其余列只给提示） */
const showCreateInEmpty = computed(() => props.columnId === 'todo' && !props.loading)

function setColumnRef(el: unknown): void {
  emit('register-column', el instanceof HTMLElement ? el : null)
}
</script>

<template>
  <section
    class="flex min-w-[280px] flex-1 flex-col rounded-md border border-neutral-200 bg-neutral-50/60 transition-colors"
    :class="dropHover && 'border-blue-400 bg-blue-50/40'"
    :data-column-id="columnId"
  >
    <!-- 列头：标题 + 计数 -->
    <header class="flex shrink-0 items-center gap-1.5 px-2.5 pt-2 pb-1.5">
      <span class="text-[13px] font-medium text-neutral-700">{{ label }}</span>
      <span
        class="rounded-full bg-neutral-200/70 px-1.5 py-px text-[10px] leading-4 text-neutral-500"
      >
        {{ tasks.length }}
      </span>
      <span class="flex-1" />
    </header>

    <!-- 列体：滚动容器；拖拽落列按此矩形判定（registerColumn） -->
    <div :ref="setColumnRef" class="min-h-0 flex-1 overflow-y-auto px-1.5 pb-2">
      <!-- 加载骨架 -->
      <template v-if="loading">
        <div
          v-for="i in 2"
          :key="`skeleton-${i}`"
          class="mb-1.5 animate-pulse rounded-md border border-neutral-200 bg-white px-2.5 py-2"
        >
          <div class="h-3 w-14 rounded bg-neutral-200/70" />
          <div class="mt-2 h-3 w-full rounded bg-neutral-200/60" />
          <div class="mt-1 h-3 w-2/3 rounded bg-neutral-200/60" />
        </div>
      </template>

      <!-- 空态 -->
      <div
        v-else-if="tasks.length === 0"
        class="mt-6 flex flex-col items-center gap-2 text-center text-[11px] text-neutral-400"
      >
        <span>{{ hint }}</span>
        <button
          v-if="showCreateInEmpty"
          class="flex cursor-pointer items-center gap-1 rounded-md border border-neutral-200 bg-white px-2 py-1 text-[11px] text-neutral-600 hover:bg-neutral-50"
          @click="emit('create-task')"
        >
          <Plus class="h-3 w-3" />
          新建任务
        </button>
      </div>

      <!-- 卡片列表 -->
      <template v-else>
        <div
          v-for="task in tasks"
          :key="task.id"
          class="mb-1.5 last:mb-0"
        >
          <TaskCard
            :task="task"
            :project-name="allProjects ? (projectNameOf(task.projectId) ?? undefined) : undefined"
            :project-missing="allProjects && projectNameOf(task.projectId) === null"
            :drag-enabled="dragEnabled"
            :dragging="draggingTaskId === task.id"
            :shaking="shakingTaskId === task.id"
            :busy="busyTaskIds.has(task.id)"
            :primary-label="primaryLabelOf(task)"
            :secondary-label="secondaryLabelOf(task)"
            @element="(el) => emit('register-card', task.id, el)"
            @pointer-down="(e) => emit('card-pointer-down', e, task)"
            @open="emit('card-open', task)"
            @primary="emit('card-primary', task)"
            @secondary="emit('card-secondary', task)"
          />
        </div>
      </template>
    </div>
  </section>
</template>
