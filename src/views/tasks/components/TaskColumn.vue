<script setup lang="ts">
// 看板列：竖色条 + 标题 + 计数胶囊 + 卡片列表 + 空态虚线框。
// 列本身无背景、无边框（codeg tasks-page.tsx:817-880），白底看板靠列间 gap 分隔；
// 列体是滚动容器（拖拽落列按它的矩形判定），data-column-id 供拖拽 composable 注册；
// 高亮只表示「合法落列」（PRD R3），非法落列不亮。
import { computed } from 'vue'
import { Plus } from 'lucide-vue-next'
import TaskCard from './TaskCard.vue'
import { columnLabel, emptyColumnHint } from '../boardColumns'
import type { BoardColumnId } from '../boardColumns'
import type { WorkTask } from '../../../types/task'

const props = defineProps<{
  columnId: BoardColumnId
  tasks: WorkTask[]
  /** 项目筛选（null = 全部项目：卡片显示项目名） */
  allProjects: boolean
  /** 拖拽是否启用（当前恒为 true） */
  dragEnabled: boolean
  /** 同列排序是否可用（全部项目视图 = false，只允许跨列） */
  reorderEnabled: boolean
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

/** 列头竖色条：四列互不相同，颜色只表达阶段性质（design §2.1） */
const accentClass = computed(() => {
  switch (props.columnId) {
    case 'todo':
      return 'bg-neutral-400'
    case 'inProgress':
      return 'bg-blue-500'
    case 'attention':
      return 'bg-amber-500'
    case 'done':
      return 'bg-green-500'
    default: {
      const unhandled: never = props.columnId
      return unhandled
    }
  }
})

/** 计数胶囊：等你处理列有内容时用琥珀底强调，其余灰底（design §2.1） */
const countClass = computed(() =>
  props.columnId === 'attention' && props.tasks.length > 0
    ? 'bg-amber-50 text-amber-600'
    : 'bg-neutral-100 text-neutral-500',
)

/** 待办列空态额外给「新建任务」入口（其余列只给提示） */
const showCreateInEmpty = computed(() => props.columnId === 'todo' && !props.loading)

function setColumnRef(el: unknown): void {
  emit('register-column', el instanceof HTMLElement ? el : null)
}
</script>

<template>
  <section
    :ref="setColumnRef"
    class="flex min-w-[280px] flex-1 flex-col gap-2 rounded-xl transition-colors"
    :class="dropHover && 'bg-blue-50/40 ring-2 ring-blue-400'"
    :data-column-id="columnId"
  >
    <!-- 列头：3px 竖色条 + 标题 + 计数胶囊，高 24px（对齐 codeg） -->
    <header class="flex h-6 shrink-0 items-center gap-2 px-0.5">
      <span class="h-3.5 w-[3px] shrink-0 rounded-full" :class="accentClass" />
      <h2 class="text-xs font-semibold text-neutral-700">{{ label }}</h2>
      <span
        class="rounded-full px-1.5 py-0.5 text-[10px] font-medium leading-none tabular-nums"
        :class="countClass"
      >
        {{ tasks.length }}
      </span>
    </header>

    <!-- 加载骨架 -->
    <div v-if="loading" class="min-h-0 flex-1 space-y-3 overflow-y-auto">
      <div
        v-for="i in 2"
        :key="`skeleton-${i}`"
        class="animate-pulse rounded-xl border border-neutral-200 bg-white p-3"
      >
        <div class="h-3 w-full rounded bg-neutral-200/70" />
        <div class="mt-2 h-3 w-2/3 rounded bg-neutral-200/60" />
      </div>
    </div>

    <!-- 空态：虚线框占满列体，不用灰底（codeg 同款） -->
    <div
      v-else-if="tasks.length === 0"
      class="flex flex-1 flex-col items-center justify-center gap-1.5 rounded-xl border border-dashed border-neutral-300 p-4 text-center"
    >
      <p class="text-xs text-neutral-400">{{ hint }}</p>
      <button
        v-if="showCreateInEmpty"
        class="mt-0.5 flex cursor-pointer items-center gap-1 rounded-md px-2 py-1 text-[11px] text-neutral-500 transition-colors hover:bg-neutral-100 hover:text-neutral-700"
        @click="emit('create-task')"
      >
        <Plus class="h-3 w-3" />
        新建任务
      </button>
    </div>

    <!-- 卡片列表：滚动容器，拖拽落列按整列矩形判定（registerColumn 挂在 section 上，
         空列也必须是合法落点——把待办卡片拖进空的「进行中」列正是最常见的操作）。
         与外层空态同一套虚线框，保证四列在有/无卡片时视觉一致。 -->
    <div
      v-else
      class="min-h-0 flex-1 space-y-3 overflow-y-auto rounded-xl border border-dashed border-neutral-300 p-2"
    >
      <TaskCard
        v-for="task in tasks"
        :key="task.id"
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
  </section>
</template>
