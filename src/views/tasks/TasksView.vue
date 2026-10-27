<script setup lang="ts">
// 任务看板页（/tasks/:projectId）：工具栏（当前项目 / 隐藏开关 / 新建）+ 四列横向布局。
// 项目来自路由参数（projectId === 'all' 为全部项目视图），不再读 localStorage 筛选。
// 数据：taskStore（唯一来源，异步变更 await 后重读返回值）；项目名：projectStore。
// 拖拽：useTaskDrag（Pointer Events），drop 时经本页调 taskStore.transition / reorder。
// 会话联动（子任务 3）：开始 / 重试 / 停止 / 通过 / 打回全部委托 taskSessionBridge
// （启动链路唯一实现），本页只负责提示与在途禁用。
// worktree（子任务 lifecycle）：拖进「已完成」列 = 进终态，同样要删工作树，故复用
// taskSessionBridge 的 `transitionTask` 而不是直接调 store。
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useMessage } from 'naive-ui'
import { ArrowLeft, ChevronLeft, FolderOpen, ListTodo, Plus } from 'lucide-vue-next'
import TaskColumn from './components/TaskColumn.vue'
import WindowControls from '../coding/components/WindowControls.vue'
import TaskEditorDialog from './components/TaskEditorDialog.vue'
import TaskDetailDialog from './components/TaskDetailDialog.vue'
import { useTaskDrag } from './components/useTaskDrag'
import {
  BOARD_COLUMN_IDS,
  STATUSES_BY_COLUMN as COLUMN_STATUSES,
  columnLabel,
  columnForStatus,
  groupTasksByColumn,
} from './boardColumns'
import { approveTask, rejectTask, startTask, stopTask, transitionTask } from './taskSessionBridge'
import { useTaskStore } from '../../stores/task'
import { useProjectStore } from '../../stores/project'
import { taskErrorMessage } from '../../types/task'
import type { WorkTask } from '../../types/task'
import type { EngineWire } from '../../types/engine'
import type { BoardColumnId } from './boardColumns'

const props = defineProps<{
  /** 路由参数：具体项目 id，或 'all'（全部项目） */
  projectId: string
}>()

const router = useRouter()
const message = useMessage()
const taskStore = useTaskStore()
const projectStore = useProjectStore()

// ---- 项目（唯一来源 = 路由参数，prd R3） ----

/** 全部项目视图保留 id（与 ProjectPickerView 的「全部项目」卡片一致） */
const ALL_PROJECTS_ID = 'all'

/** 具体的项目 id（全部项目视图为 null）；拖拽排序语义按它判断 */
const scopedProjectId = computed<string | null>(() =>
  props.projectId === ALL_PROJECTS_ID ? null : props.projectId,
)

/** 是否全部项目视图（卡片显示项目名、禁止同列排序） */
const allProjects = computed(() => scopedProjectId.value === null)

/** 路由指向的项目已被移除（且不是 all）→ 看板换成「项目已移除」空态 */
const projectMissing = computed(
  () => scopedProjectId.value !== null && projectStore.get(scopedProjectId.value) === null,
)

const boardTitle = computed(() =>
  scopedProjectId.value === null ? '全部项目' : (projectStore.get(scopedProjectId.value)?.name ?? '项目已移除'),
)

// ---- 显示开关（localStorage 记忆，同步读取保证首帧就是用户上次的选择） ----

const SHOW_CANCELED_KEY = 'lightr.tasks.showCanceled'
const SHOW_ARCHIVED_KEY = 'lightr.tasks.showArchived'

function readBool(key: string, fallback: boolean): boolean {
  try {
    const raw = window.localStorage.getItem(key)
    if (raw === null) return fallback
    return raw === 'true'
  } catch {
    return fallback
  }
}

function writeBool(key: string, value: boolean): void {
  try {
    window.localStorage.setItem(key, String(value))
  } catch {
    // 隐私模式 / 配额满：记忆失败不影响当前会话
  }
}

const showCanceled = ref(readBool(SHOW_CANCELED_KEY, false))
const showArchived = ref(readBool(SHOW_ARCHIVED_KEY, false))

function toggleShowCanceled(): void {
  showCanceled.value = !showCanceled.value
  writeBool(SHOW_CANCELED_KEY, showCanceled.value)
}

function toggleShowArchived(): void {
  showArchived.value = !showArchived.value
  writeBool(SHOW_ARCHIVED_KEY, showArchived.value)
}

// ---- 分组（唯一派生入口） ----

const visibleTasks = computed(() => {
  const projectId = scopedProjectId.value
  if (projectId === null) return taskStore.tasks
  return taskStore.tasks.filter((t) => t.projectId === projectId)
})

const grouped = computed(() =>
  groupTasksByColumn(visibleTasks.value, {
    showCanceled: showCanceled.value,
    showArchived: showArchived.value,
  }),
)

/**
 * 最终渲染用的分组：拖拽同列期间用拖拽 composable 的本地顺序覆盖被拖那列，
 * 其余列保持 store 顺序。松手后 localOrder 归 null，回到 store 权威顺序。
 */
const renderGrouped = computed<Record<BoardColumnId, WorkTask[]>>(() => {
  const base = grouped.value
  const order = localOrder.value
  if (!order || order.length === 0) return base
  const wanted = new Map(order.map((id, index) => [id, index]))
  const next: Record<BoardColumnId, WorkTask[]> = { ...base }
  for (const columnId of BOARD_COLUMN_IDS) {
    const list = base[columnId]
    // 只覆盖「本地顺序恰好就是这一列可见卡片集合」的列；否则（拖拽中状态被后台改掉、
    // 或列表因筛选变化）保留 store 顺序，避免出现缺项 / 串列。
    if (list.length !== order.length || !list.every((t) => wanted.has(t.id))) continue
    next[columnId] = [...list].sort((a, b) => (wanted.get(a.id) ?? 0) - (wanted.get(b.id) ?? 0))
  }
  return next
})

const totalVisible = computed(() =>
  BOARD_COLUMN_IDS.reduce((sum, id) => sum + grouped.value[id].length, 0),
)

/** 项目名解析：全部项目视图显示；缺失 = 项目已移除（prd PAC4） */
function projectNameOf(projectId: string): string | null {
  return projectStore.get(projectId)?.name ?? null
}

// ---- 动作矩阵（PRD R4）：主 / 次按钮文案，按状态穷举 ----

/** 正在转移中的任务 id：按钮置灰，避免双击发出第二次转移（第二次必然 stale） */
const inflight = reactive(new Set<string>())

function primaryLabelOf(task: WorkTask): string | null {
  switch (task.status) {
    case 'todo':
      return '开始'
    case 'queued':
    case 'preparing':
    case 'running':
    case 'awaiting_input':
      return '停止'
    case 'review':
      return '通过'
    case 'failed':
      return '重试'
    case 'done':
    case 'canceled':
    case 'merging':
      return null
    default: {
      const unhandled: never = task.status
      return unhandled
    }
  }
}

function secondaryLabelOf(task: WorkTask): string | null {
  switch (task.status) {
    case 'review':
      return '打回'
    case 'todo':
    case 'queued':
    case 'preparing':
    case 'running':
    case 'awaiting_input':
    case 'failed':
    case 'done':
    case 'canceled':
    case 'merging':
      return null
    default: {
      const unhandled: never = task.status
      return unhandled
    }
  }
}

/** 每个任务的动作（子任务 3）：开始 / 停止 / 通过 / 打回 / 重试全部走会话联动的
 *  唯一实现（taskSessionBridge），不再只是改状态——「开始」会建会话并发出提示词。
 *  统一在途禁用：双击不会被处理两次（第二次必然 stale）。 */
async function withInflight(task: WorkTask, action: () => Promise<void>): Promise<void> {
  if (inflight.has(task.id)) return
  inflight.add(task.id)
  try {
    await action()
  } catch (err) {
    message.error(taskErrorMessage(err))
  } finally {
    inflight.delete(task.id)
  }
}

/** 唯一动作分发表：状态 → 主/次按钮对应的行为 */
async function onCardPrimary(task: WorkTask): Promise<void> {
  switch (task.status) {
    case 'todo':
      await withInflight(task, () => startTask(task.id))
      return
    case 'queued':
    case 'preparing':
    case 'running':
    case 'awaiting_input':
      await withInflight(task, () => stopTask(task))
      return
    case 'review':
      await withInflight(task, () => approveTask(task))
      return
    case 'failed':
      // 重试 = 同一条启动链路（failed → queued → 复用会话）
      await withInflight(task, () => startTask(task.id))
      return
    default:
      return
  }
}

async function onCardSecondary(task: WorkTask): Promise<void> {
  if (task.status !== 'review') return
  await withInflight(task, () => rejectTask(task))
}

// ---- 拖拽 ----

/** 当前项目（composable 用 computed 判断是否允许同列排序） */
const scopedProjectRef = computed(() => scopedProjectId.value)

const {
  drag,
  hoverColumn,
  shakingTaskId,
  localOrder,
  dragEnabled,
  reorderEnabled,
  registerCard,
  registerColumn,
  onCardPointerDown,
  shouldSuppressClick,
  resetDrag,
} = useTaskDrag({
  projectId: scopedProjectRef,
  onMoveToStatus: (taskId, to, from) => {
    const task = taskStore.get(taskId)
    if (!task) return
    if (inflight.has(taskId)) return
    // 「待办 → 进行中」不是一次状态转移，而是启动整条会话链路：
    // 只写 queued 会让任务停在排队中永远不执行（卡片还留在待办列）。
    // 其余跨列转移（通过 / 打回）仍是纯状态变更。
    if (to === 'queued') {
      void withInflight(task, () => startTask(taskId))
      return
    }
    inflight.add(taskId)
    void transitionTask(taskId, to, from)
      .catch((err: unknown) => {
        message.error(taskErrorMessage(err))
      })
      .finally(() => {
        inflight.delete(taskId)
      })
  },
  onReorder: (orderedIds) => {
    const projectId = scopedProjectId.value
    if (projectId === null) return
    // 只提交新顺序；本项目该列里被当前筛选隐藏的行（showCanceled / showArchived 关掉时）
    // 按原 sortOrder 追加到末尾，否则重排会把这些行挤出列表顺序（它们下次显示时顺序错乱）。
    const visible = new Set(orderedIds)
    const hidden = taskStore
      .byProject(projectId)
      .filter((t) => !visible.has(t.id) && COLUMN_STATUSES.todo.includes(t.status))
      .sort((a, b) => a.sortOrder - b.sortOrder)
      .map((t) => t.id)
    void taskStore.reorder(projectId, [...orderedIds, ...hidden]).catch((err: unknown) => {
      message.error(taskErrorMessage(err))
    })
  },
})

/** 被拖拽卡片 id（原卡片降透明度） */
const draggingTaskId = computed(() => (drag.value?.active ? drag.value.taskId : null))

/** 卡片点击：拖拽刚结束的 300ms 内抑制（design §5），否则打开详情抽屉 */
function onCardOpen(task: WorkTask): void {
  if (shouldSuppressClick()) return
  detailTaskId.value = task.id
  detailOpen.value = true
}

// ---- 新建 / 编辑任务（TaskEditorDialog：新建与编辑同一个表单） ----

const editorOpen = ref(false)
/** 编辑中的任务（null = 新建） */
const editingTask = ref<WorkTask | null>(null)

/** 新建时的默认项目：具体项目看板预选并锁定；全部项目 / 卡片页需手动选（prd R4） */
const defaultProjectId = computed(() => scopedProjectId.value)

function openCreate(): void {
  editingTask.value = null
  editorOpen.value = true
}

/** 详情抽屉 / 卡片调用的编辑入口 */
function openEdit(task: WorkTask): void {
  if (task.status !== 'todo' && task.status !== 'failed') {
    message.warning('只有待办 / 失败的任务可以编辑')
    return
  }
  editingTask.value = task
  detailOpen.value = false
  editorOpen.value = true
}

async function onEditorSubmit(payload: {
  projectId: string
  title: string
  prompt: string
  engineId: EngineWire
  modelId: string | null
  effortId: string | null
  effortValue: string | null
  useWorktree: boolean
  baseBranch: string | null
  initCommand: string | null
}): Promise<void> {
  try {
    if (editingTask.value) {
      await taskStore.update(editingTask.value.id, {
        title: payload.title,
        prompt: payload.prompt,
        engineId: payload.engineId,
        modelId: payload.modelId,
        effortId: payload.effortId,
        effortValue: payload.effortValue,
        useWorktree: payload.useWorktree,
        baseBranch: payload.baseBranch,
        initCommand: payload.initCommand,
      })
    } else {
      await taskStore.create({
        projectId: payload.projectId,
        title: payload.title,
        prompt: payload.prompt,
        engineId: payload.engineId,
        modelId: payload.modelId,
        effortId: payload.effortId,
        effortValue: payload.effortValue,
        useWorktree: payload.useWorktree,
        baseBranch: payload.baseBranch,
        initCommand: payload.initCommand,
      })
    }
    editorOpen.value = false
    editingTask.value = null
  } catch (err) {
    message.error(taskErrorMessage(err))
  }
}

// ---- 详情抽屉 ----

const detailOpen = ref(false)
const detailTaskId = ref<string | null>(null)

/** 关闭详情：清空 id，避免下次打开闪现上一个任务 */
function closeDetail(): void {
  detailOpen.value = false
  detailTaskId.value = null
}

// ---- 生命周期 ----

onMounted(async () => {
  if (!projectStore.loaded) {
    try {
      await projectStore.load()
    } catch {
      // 项目索引读取失败：看板仍可用，全部项目视图下卡片会标「项目已移除」
    }
  }
  if (!taskStore.loaded) await taskStore.load()
})

// 拖拽「开始 / 重试」走 taskSessionBridge：失败时它自己提示（不抛出），成功时
// 由 bridge 直接改 store，本页无需再处理返回值。

/**
 * 路由参数变化（/tasks/a → /tasks/b）时组件会被复用：中止进行中的拖拽（否则会残留
 * 窗口监听与本地顺序）、重置在途集合，关闭详情 / 编辑器，避免状态串到新看板。
 * 卡片 / 列 DOM 注册表由 ref 回调在重渲染时自然重建，这里不去动它。
 */
watch(
  () => props.projectId,
  () => {
    resetDrag()
    closeDetail()
    editorOpen.value = false
    editingTask.value = null
    inflight.clear()
  },
)

/** 返回项目选择页：仅在「项目已移除」这种无法继续的场景下使用 */
function goBack(): void {
  void router.push('/tasks')
}
</script>

<template>
  <div class="flex h-full flex-col bg-white">
    <!-- 顶部拖拽条：返回 + 标题 + 窗口三键（无边框窗口，浏览器环境隐藏） -->
    <div
      data-drag-region
      class="flex h-10 shrink-0 items-center justify-between border-b border-neutral-200 px-2"
    >
      <div class="flex items-center gap-1.5" data-no-drag>
        <button
          class="flex h-7 w-7 cursor-pointer items-center justify-center rounded-md text-neutral-500 transition-colors hover:bg-neutral-100 hover:text-neutral-700"
          title="返回项目列表"
          @click="goBack"
        >
          <ArrowLeft class="h-4 w-4" />
        </button>
        <ListTodo class="h-4 w-4 text-neutral-400" />
        <span class="min-w-0 max-w-[280px] truncate text-[13px] font-medium text-neutral-800">
          {{ boardTitle }}
        </span>
      </div>
      <WindowControls />
    </div>

    <!-- 工具栏：显示开关 + 计数 + 新建（项目由路由确定，不做返回/切换） -->
    <div class="flex shrink-0 items-center gap-2 border-b border-neutral-200 px-3 py-2">
      <!-- 显示开关（checkbox + 文字，原生控件以保持轻量） -->
      <label class="flex cursor-pointer items-center gap-1 text-[12px] text-neutral-600">
        <input
          type="checkbox"
          class="h-3 w-3 cursor-pointer accent-blue-500"
          :checked="showCanceled"
          @change="toggleShowCanceled"
        />
        隐藏已取消
      </label>
      <label class="flex cursor-pointer items-center gap-1 text-[12px] text-neutral-600">
        <input
          type="checkbox"
          class="h-3 w-3 cursor-pointer accent-blue-500"
          :checked="showArchived"
          @change="toggleShowArchived"
        />
        隐藏已归档
      </label>

      <span class="flex-1" />

      <!-- 全部项目视图可跨列拖动，但同列排序没有项目内语义：显式说明 -->
      <span v-if="!reorderEnabled" class="text-[11px] text-neutral-400">
        可拖到其它列推进状态 · 排序需选择具体项目
      </span>
      <span v-else class="text-[11px] text-neutral-400">共 {{ totalVisible }} 个任务</span>

      <button
        class="flex h-7 cursor-pointer items-center gap-1 rounded-full bg-neutral-900 px-3 text-[12px] text-white transition-colors hover:bg-neutral-800 disabled:cursor-not-allowed disabled:opacity-40"
        :disabled="projectStore.projects.length === 0"
        @click="openCreate"
      >
        <Plus class="h-3.5 w-3.5" />
        新建任务
      </button>
    </div>

    <!-- 四列看板：横向 flex，列最小 280px；窄屏横向滚动。列白底无边框，靠 gap 分隔 -->
    <!-- 路由指向的项目已被移除：给出口而不是空白看板（prd AC10） -->
    <main
      v-if="projectMissing"
      class="flex min-h-0 flex-1 flex-col items-center justify-center gap-2 bg-white p-4 text-center"
    >
      <FolderOpen class="h-6 w-6 text-neutral-300" />
      <p class="text-xs text-neutral-400">项目已移除</p>
      <button
        class="flex h-7 cursor-pointer items-center gap-1 rounded-md border border-neutral-200 px-2.5 text-[12px] text-neutral-600 transition-colors hover:bg-neutral-100"
        @click="goBack"
      >
        <ChevronLeft class="h-3.5 w-3.5" />
        返回项目列表
      </button>
    </main>
    <main v-else class="flex min-h-0 flex-1 gap-4 overflow-x-auto bg-white p-4">
      <TaskColumn
        v-for="columnId in BOARD_COLUMN_IDS"
        :key="columnId"
        :column-id="columnId"
        :tasks="renderGrouped[columnId]"
        :all-projects="allProjects"
        :drag-enabled="dragEnabled"
        :reorder-enabled="reorderEnabled"
        :dragging-task-id="draggingTaskId"
        :shaking-task-id="shakingTaskId"
        :drop-hover="hoverColumn === columnId"
        :loading="!taskStore.loaded"
        :busy-task-ids="inflight"
        :project-name-of="projectNameOf"
        :primary-label-of="primaryLabelOf"
        :secondary-label-of="secondaryLabelOf"
        @register-column="(el) => registerColumn(columnId, el)"
        @register-card="registerCard"
        @card-pointer-down="(e, task) => onCardPointerDown(e, task.id, task.status)"
        @card-open="onCardOpen"
        @card-primary="(task) => void onCardPrimary(task)"
        @card-secondary="(task) => void onCardSecondary(task)"
        @create-task="openCreate"
      />
    </main>

    <!-- 拖拽预览：Teleport 到 body（列是滚动容器会裁剪卡片），fixed 跟随指针 -->
    <Teleport to="body">
      <div
        v-if="drag && drag.active && taskStore.get(drag.taskId)"
        class="pointer-events-none fixed z-[120] rotate-1 cursor-grabbing opacity-95"
        :style="{
          left: `${drag.clientX - drag.offsetX}px`,
          top: `${drag.clientY - drag.offsetY}px`,
          width: `${drag.width}px`,
        }"
      >
        <div class="rounded-md border border-blue-300 bg-white px-2.5 py-2 shadow-md">
          <div class="flex items-center gap-1.5">
            <span
              class="inline-flex items-center rounded-full border border-neutral-200 bg-neutral-100 px-1.5 py-px text-[10px] leading-4 text-neutral-500"
            >
              {{ columnLabel(hoverColumn ?? columnForStatus(drag.status)) }}
            </span>
          </div>
          <p class="mt-1 line-clamp-2 text-[13px] leading-[18px] text-neutral-800">
            {{ taskStore.get(drag.taskId)?.title }}
          </p>
        </div>
      </div>
    </Teleport>

    <!-- 新建 / 编辑任务（同一表单） -->
    <TaskEditorDialog
      :open="editorOpen"
      :task="editingTask"
      :default-project-id="defaultProjectId"
      :project-locked="!allProjects"
      @close="editorOpen = false"
      @submit="onEditorSubmit"
    />

    <!-- 任务详情：动作区（启动 / 停止 / 通过 / 打回 / 重试 / 归档 / 删除 / 断开会话） -->
    <TaskDetailDialog
      :open="detailOpen"
      :task-id="detailTaskId"
      @close="closeDetail"
      @edit="openEdit"
    />
  </div>
</template>
