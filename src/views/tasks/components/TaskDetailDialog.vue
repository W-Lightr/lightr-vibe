<script setup lang="ts">
// 任务详情（prd R2）：头部（标题 / 状态 / 项目 / 时间）+ 正文（提示词 / 失败原因 / 结果摘要）
// + 时间线（升序，status_changed 渲染为「待办 → 进行中」）+ 按状态给全量动作。
// 动作全部委托 taskSessionBridge（启动链路唯一实现）与 task store；本组件只做
// 在途禁用、错误提示与弹窗开合，不直接碰 service。
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { NPopconfirm, useMessage } from 'naive-ui'
import {
  AlertTriangle,
  Archive,
  Check,
  Copy,
  CornerDownLeft,
  ExternalLink,
  GitBranch,
  History,
  LoaderCircle,
  Pencil,
  Play,
  RotateCcw,
  Square,
  Trash2,
  Unlink,
  X,
} from 'lucide-vue-next'
import TaskStatusChip from './TaskStatusChip.vue'
import { formatDateTime } from '../../../utils/time'
import { TASK_STATUS_LABELS, taskErrorMessage } from '../../../types/task'
import type { TaskStatus, WorkTask, WorkTaskEvent } from '../../../types/task'
import { useTaskStore } from '../../../stores/task'
import { useProjectStore } from '../../../stores/project'
import { useSessionStore } from '../../../stores/session'
import { useWorkspaceStore } from '../../../stores/workspace'
import { isTauri } from '../../../utils/isTauri'
import { SETUP_ERROR_PREFIX, WORKTREE_AUTOCOMMIT_FAILED_PREFIX, WORKTREE_CLEANUP_FAILED_PREFIX } from './setupErrorPrefix'
import {
  approveTask,
  deleteWorktreeBranch,
  rejectTask,
  retryWorktreeCleanup,
  simulateTurnSettle,
  startTask,
  stopTask,
} from '../taskSessionBridge'

const props = defineProps<{
  open: boolean
  taskId: string | null
}>()

const emit = defineEmits<{
  close: []
  /** 需要打开编辑器（父级统一开 TaskEditorDialog，避免两个弹窗各自维护状态） */
  edit: [task: WorkTask]
}>()

const router = useRouter()
const message = useMessage()
const taskStore = useTaskStore()
const projectStore = useProjectStore()
const sessionStore = useSessionStore()
const workspace = useWorkspaceStore()

const task = computed<WorkTask | null>(() => taskStore.get(props.taskId))
const events = ref<WorkTaskEvent[]>([])
const loadingEvents = ref(false)
const busy = ref(false)

/** 项目名（缺失 = 已移除，prd PAC4：仍可删除/归档） */
const project = computed(() => (task.value ? projectStore.get(task.value.projectId) : null))

/** 可编辑状态：与 Rust task_update 的放行条件一致 */
const editable = computed(() => task.value?.status === 'todo' || task.value?.status === 'failed')
/** 运行中（含排队 / 准备中）：动作区给「停止」 */
const running = computed(() => {
  const s = task.value?.status
  return s === 'queued' || s === 'preparing' || s === 'running' || s === 'awaiting_input'
})
/** 终态才可归档（与 Rust task_archive 一致） */
const archivable = computed(() => {
  const s = task.value?.status
  return s === 'done' || s === 'failed' || s === 'canceled'
})
const canDetach = computed(() => !!task.value?.sessionId && !running.value)
/** 会话是否仍在本机索引里（被删掉的会话只留一个不可点的提示） */
const sessionExists = computed(() =>
  task.value?.sessionId ? !!sessionStore.get(task.value.sessionId) : false,
)

/** 浏览器 dev：手动模拟 turn 结论的入口（仅 !isTauri() 渲染，prd R7） */
const devMode = computed(() => !isTauri())
/** 模拟按钮只在「运行中且已绑定会话」时有意义（回到待办就没有本次运行可结算） */
const canSimulate = computed(() => !!task.value?.sessionId && running.value)

/** 工作树区（子任务 10-26-worktree-editor R6）：创建过才显示，只读 + 可复制路径 */
const worktreePath = computed(() => task.value?.worktreePath ?? null)
/** 刚复制成功的键（短暂显示对勾）；定时器在关闭 / 卸载时清掉，避免弹窗销毁后回调跑空 */
const copiedKey = ref<string | null>(null)
let copiedTimer: number | null = null

async function copyValue(key: string, value: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(value)
    copiedKey.value = key
    if (copiedTimer !== null) window.clearTimeout(copiedTimer)
    copiedTimer = window.setTimeout(() => {
      copiedKey.value = null
      copiedTimer = null
    }, 1500)
  } catch {
    // 剪贴板不可用时静默忽略（与 SessionInspector 一致）：非安全上下文下可能没有该 API
  }
}

/** 基线行的展示：分支 + 短 sha（sha 可能缺失） */
const baseSummary = computed(() => {
  const t = task.value
  if (!t) return null
  const branch = t.baseBranch
  const sha = t.baseSha ? t.baseSha.slice(0, 8) : null
  if (branch && sha) return `${branch} / ${sha}`
  return branch ?? sha
})

/**
 * 工作树清理失败（lifecycle R3 / AC9）：任务已是终态、但「登记着路径」或「刚写回了清理
 * 失败附注」= 没删掉。
 *
 * 两种证据取并集：删除失败会保留路径；但删除成功而 `clearWorktree` 没写回时路径仍在——
 * 那不算清理失败，只有 `lastError` 会在那种情况写出来。所以以附注为主、路径为辅：
 * - 有路径：详情区本来就渲染（路径 / 分支 / 基线，R4），这里只补「失败 + 重试」；
 * - 无路径但附注还在：说明重试已删掉目录，只是记录没跟上，给个入口看/清掉附注。
 */
const worktreeCleanupFailed = computed(() => {
  const t = task.value
  if (!t) return false
  const terminal = t.status === 'done' || t.status === 'canceled' || t.status === 'failed'
  if (!terminal) return false
  return !!t.worktreePath || !!t.lastError?.includes(WORKTREE_CLEANUP_FAILED_PREFIX)
})

/**
 * 「删除分支」（worktree keep-branch R3 / AC5 / AC6）：任务终态后目录已删、分支按 R2 保留
 * （那是这次任务的产品）。有分支名就允许删——`worktreePath` 为 null 也算（目录已清理、
 * 只剩分支；Rust 侧 `worktree_remove` 把空路径当作「没有目录要删」）。
 *
 * 只对**已结束且目录已清**的任务显示：
 * - 运行中的任务还在用这条分支，删掉等于毁掉在途工作；
 * - `review` / `failed` 时目录通常还在（`failed` 可能还挂着启动失败的清理）、
 *   那时该走「重试清理」，不能跳过它直接把分支删掉（否则目录成孤儿）。
 */
const retainedBranch = computed(() => task.value?.workBranch ?? null)
const canDeleteBranch = computed(() => {
  const s = task.value?.status
  const terminal = s === 'done' || s === 'canceled'
  return !!retainedBranch.value && terminal && !task.value?.worktreePath
})

/**
 * 自动提交的结果（task 10-26-worktree-autocommit / R4）。
 *
 * `unknown` = 这条任务跑的时候还没有自动提交（或还没清理，工作区里可能仍有未提交改动），
 * 详情不额外提示；`ok` = 终态清理时已经先提交再删目录，成果在分支上；
 * `failed` = 提交失败（如缺 git 身份），未提交的改动随目录一起丢了。
 *
 * 只用 `lastError` 的附注判定：成功路径**不写任何记录**（R2「静默跳过」），所以
 * 「终态 + 分支保留 + 没有失败附注」就是成功。工作树清理失败的附注里也可能带着自动
 * 提交失败（先提交、后删除，两步各自独立），两种都按失败处理。
 */
const worktreeAutocommit = computed<'failed' | 'ok' | 'unknown'>(() => {
  const t = task.value
  if (!t) return 'unknown'
  const detail = t.lastError ?? ''
  if (detail.includes(WORKTREE_AUTOCOMMIT_FAILED_PREFIX)) return 'failed'
  const terminal = t.status === 'done' || t.status === 'canceled'
  if (terminal && t.workBranch && !t.worktreePath) return 'ok'
  return 'unknown'
})

/** 失败原因：截掉前缀与后缀括号，只留 `git commit: …` 这类原因 */
const worktreeAutocommitReason = computed(() => {
  const detail = task.value?.lastError ?? ''
  const at = detail.indexOf(WORKTREE_AUTOCOMMIT_FAILED_PREFIX)
  if (at === -1) return ''
  const rest = detail.slice(at + WORKTREE_AUTOCOMMIT_FAILED_PREFIX.length)
  return rest.replace(/（未提交的改动已丢失）[\s\S]*$/, '').trim() || rest.trim()
})

/** 删除分支：二次确认由模板里的 NPopconfirm 负责，这里只执行（失败由 run() toast） */
function onDeleteBranch(): void {
  const t = task.value
  if (!t) return
  void run(async () => {
    await deleteWorktreeBranch(t.id)
    message.success(`分支 ${retainedBranch.value ?? ''} 已删除`)
  })
}

/** 「失败原因」区：工作树附注不是 Agent 失败原因，已在工作树区单独说明，别重复红块 */
const showLastError = computed(() => {
  const message = task.value?.lastError
  if (!message) return false
  return !message.startsWith(SETUP_ERROR_PREFIX)
})

/** 重试清理：成功后路径消失、卡片自动收起；失败由 run() 统一 toast（lastError 已写回） */
function onRetryWorktreeCleanup(): void {
  const t = task.value
  if (!t) return
  void run(() => retryWorktreeCleanup(t.id))
}

/** 时间线事件文案：状态事件渲染成「待办 → 进行中」 */
function eventLabel(event: WorkTaskEvent): string {
  switch (event.kind) {
    case 'status_changed': {
      const from = event.payload?.from
      const to = event.payload?.to
      const fromLabel =
        typeof from === 'string' ? (TASK_STATUS_LABELS[from as TaskStatus] ?? from) : '?'
      const toLabel = typeof to === 'string' ? (TASK_STATUS_LABELS[to as TaskStatus] ?? to) : '?'
      return `${fromLabel} → ${toLabel}`
    }
    case 'created':
      return '创建任务'
    case 'user_action': {
      const action = event.payload?.action
      switch (action) {
        case 'attach_session':
          return '绑定会话'
        case 'detach_session':
          return '断开会话'
        case 'archive':
          return '归档'
        case 'unarchive':
          return '取消归档'
        case 'updated':
          return '编辑任务'
        default:
          return '操作'
      }
    }
    case 'agent_progress':
      return 'Agent 进展'
    case 'agent_verdict':
      return 'Agent 结论'
    default: {
      const unhandled: never = event.kind
      return unhandled
    }
  }
}

/** 状态事件拆成前后两段（模板里分别着色） */
function statusParts(event: WorkTaskEvent): { from: string; to: string } | null {
  if (event.kind !== 'status_changed') return null
  const from = event.payload?.from
  const to = event.payload?.to
  if (typeof from !== 'string' || typeof to !== 'string') return null
  return {
    from: TASK_STATUS_LABELS[from as TaskStatus] ?? from,
    to: TASK_STATUS_LABELS[to as TaskStatus] ?? to,
  }
}

/** 事件来源中文名 */
function actorLabel(event: WorkTaskEvent): string {
  switch (event.actor) {
    case 'user':
      return '我'
    case 'engine':
      return '引擎'
    case 'agent':
      return 'Agent'
    default: {
      const unhandled: never = event.actor
      return unhandled
    }
  }
}

async function loadEvents(): Promise<void> {
  const id = props.taskId
  if (!id) {
    events.value = []
    return
  }
  loadingEvents.value = true
  try {
    // 升序（Rust 已排序，这里按 createdAt 再稳一次，AC6）
    events.value = [...(await taskStore.eventsOf(id))].sort((a, b) => a.createdAt - b.createdAt)
  } catch {
    events.value = []
  } finally {
    loadingEvents.value = false
  }
}

/** 打开 / 切换任务时重读时间线（时间线随每次操作增长，不缓存） */
watch(
  () => [props.open, props.taskId] as const,
  ([open]) => {
    // 换任务时上一张卡片的「已复制」对勾不能带过来
    copiedKey.value = null
    if (open) void loadEvents()
  },
  { immediate: true },
)

/**
 * 任务行变化时重读时间线：状态变化（本组件动作、看板动作、**turn 回流**）都会写事件，
 * 只监听打开时机的话，Agent 结束时抽屉里的时间线会停在旧快照。
 */
watch(
  () => task.value?.updatedAt,
  (now, before) => {
    if (!props.open || now === before) return
    void loadEvents()
  },
)

/** 统一包一层在途保护：双击不产生第二次转移；失败用中文提示 */
async function run(action: () => Promise<void> | void): Promise<void> {
  if (busy.value) return
  busy.value = true
  try {
    await action()
    // 动作成功后时间线有新事件（后端写入），重读一次
    await loadEvents()
  } catch (err) {
    message.error(taskErrorMessage(err))
  } finally {
    busy.value = false
  }
}

function onStart(): void {
  const t = task.value
  if (!t) return
  void run(() => startTask(t.id))
}

function onStop(): void {
  const t = task.value
  if (!t) return
  void run(() => stopTask(t))
}

function onApprove(): void {
  const t = task.value
  if (!t) return
  void run(() => approveTask(t))
}

function onReject(): void {
  const t = task.value
  if (!t) return
  void run(() => rejectTask(t))
}

/** 重试 = 同一条启动链路；已有 sessionId 时复用（session/load 续上原会话，R6） */
function onRetry(): void {
  const t = task.value
  if (!t) return
  void run(() => startTask(t.id))
}

function onEdit(): void {
  if (task.value) emit('edit', task.value)
}

function onDuplicate(): void {
  const t = task.value
  if (!t) return
  void run(async () => {
    const copy = await taskStore.duplicate(t.id)
    message.success(`已复制为「${copy.title}」`)
  })
}

function onArchive(): void {
  const t = task.value
  if (!t) return
  void run(async () => {
    await taskStore.archive(t.id, t.archivedAt === null)
  })
}

function onDetach(): void {
  const t = task.value
  const sessionId = t?.sessionId
  if (!t || !sessionId) return
  void run(async () => {
    await taskStore.detachSession(t.id, sessionId)
    message.success('已断开会话，下次启动会新建')
  })
}

function onDelete(): void {
  const t = task.value
  if (!t) return
  void run(async () => {
    await taskStore.remove(t.id)
    message.success('任务已删除')
    emit('close')
  })
}

/** 查看会话：打开标签并跳到编码页 */
function openSession(): void {
  const t = task.value
  if (!t?.sessionId) return
  workspace.openSession(t.sessionId)
  void router.push('/coding')
}

/**
 * 浏览器 dev：手动模拟 turn 结论（仅 !isTauri() 显示，prd R7）。
 *
 * 「模拟完成」在这里**先给会话补一条 assistant 正文**——真实路径的 ok 回调一定带着
 * 本 turn 的产出（bridge 用「有内容」区分正常完成与用户停止，R4），浏览器里没有真
 * Agent 生成这条消息，补上之后模拟才与真实路径等价。其余三种（停止 / 空回复 / 报错）
 * 不需要内容。
 */
function simulate(outcome: 'ok' | 'stop' | 'empty' | 'error'): void {
  const t = task.value
  if (!t?.sessionId) return
  const sessionId = t.sessionId
  if (outcome === 'ok') {
    sessionStore.pushMessage(sessionId, {
      role: 'assistant',
      type: 'text',
      content: '（模拟）Agent 已完成任务并返回结果。',
      status: 'done',
    })
  }
  simulateTurnSettle(t, outcome)
}

function onKeydown(e: KeyboardEvent): void {
  if (e.key === 'Escape' && props.open) emit('close')
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown)
  // 复制成功的对勾定时器随组件一起回收（弹出的详情抽屉会反复挂载 / 卸载）
  if (copiedTimer !== null) window.clearTimeout(copiedTimer)
  copiedTimer = null
})
</script>

<template>
  <div
    v-if="props.open && task"
    data-no-drag
    class="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto bg-neutral-900/20 py-10"
    @mousedown.self="emit('close')"
  >
    <div class="w-[640px] rounded-md border border-neutral-200 bg-white">
      <!-- 头部 -->
      <div class="flex items-start gap-2 border-b border-neutral-200 px-4 py-3">
        <div class="min-w-0 flex-1">
          <div class="flex items-center gap-2">
            <TaskStatusChip :task="task" size="md" />
            <span class="truncate text-[11px] text-neutral-500">
              {{ project?.name ?? '项目已移除' }}
            </span>
            <span
              v-if="task.archivedAt !== null"
              class="rounded bg-neutral-100 px-1 py-px text-[10px] text-neutral-400"
            >
              已归档
            </span>
          </div>
          <h2 class="mt-1.5 text-[15px] font-medium leading-5 text-neutral-800">{{ task.title }}</h2>
        </div>
        <button
          class="flex h-6 w-6 shrink-0 cursor-pointer items-center justify-center rounded text-neutral-400 hover:bg-neutral-100 hover:text-neutral-600"
          title="关闭"
          @click="emit('close')"
        >
          <X class="h-3.5 w-3.5" />
        </button>
      </div>

      <div class="max-h-[calc(100vh-240px)] overflow-y-auto px-4 py-3">
        <!-- 时间戳 -->
        <dl class="grid grid-cols-2 gap-x-4 gap-y-1 text-[11px]">
          <div class="flex gap-1">
            <dt class="shrink-0 text-neutral-500">创建</dt>
            <dd class="text-neutral-700">{{ formatDateTime(task.createdAt) }}</dd>
          </div>
          <div class="flex gap-1">
            <dt class="shrink-0 text-neutral-500">开始</dt>
            <dd class="text-neutral-700">
              {{ task.startedAt ? formatDateTime(task.startedAt) : '—' }}
            </dd>
          </div>
          <div class="flex gap-1">
            <dt class="shrink-0 text-neutral-500">结算</dt>
            <dd class="text-neutral-700">
              {{ task.settledAt ? formatDateTime(task.settledAt) : '—' }}
            </dd>
          </div>
          <div class="flex gap-1">
            <dt class="shrink-0 text-neutral-500">完成</dt>
            <dd class="text-neutral-700">
              {{ task.finishedAt ? formatDateTime(task.finishedAt) : '—' }}
            </dd>
          </div>
        </dl>

        <!-- 失败原因（启动准备失败已由工作树区 / chip 表达，这里不重复） -->
        <section v-if="showLastError" class="mt-3">
          <h3 class="mb-1 text-[11px] font-medium text-neutral-500">失败原因</h3>
          <p
            class="whitespace-pre-wrap break-words rounded-md border border-red-200 bg-red-50 px-2.5 py-2 text-[12px] leading-5 text-red-700"
          >
            {{ task.lastError }}
          </p>
        </section>

        <!-- 结果摘要 -->
        <section v-if="task.resultSummary" class="mt-3">
          <h3 class="mb-1 text-[11px] font-medium text-neutral-500">结果摘要</h3>
          <p
            class="whitespace-pre-wrap break-words rounded-md border border-neutral-200 bg-neutral-50 px-2.5 py-2 text-[12px] leading-5 text-neutral-700"
          >
            {{ task.resultSummary }}
          </p>
        </section>

        <!-- 提示词 -->
        <section class="mt-3">
          <h3 class="mb-1 text-[11px] font-medium text-neutral-500">提示词</h3>
          <pre
            class="max-h-56 overflow-y-auto whitespace-pre-wrap break-words rounded-md border border-neutral-200 bg-neutral-50 px-2.5 py-2 font-sans text-[12px] leading-5 text-neutral-700"
          >{{ task.prompt || '（空）' }}</pre>
        </section>

        <!-- 会话 -->
        <section class="mt-3 flex items-center gap-2 rounded-md border border-neutral-200 px-2.5 py-2">
          <span class="shrink-0 text-[11px] text-neutral-500">会话</span>
          <span class="min-w-0 flex-1 truncate text-[12px] text-neutral-700">
            {{
              task.sessionId
                ? sessionExists
                  ? task.sessionId
                  : `${task.sessionId}（已删除）`
                : '尚未绑定'
            }}
          </span>
          <button
            v-if="task.sessionId && sessionExists"
            class="flex h-6 cursor-pointer items-center gap-1 rounded-md border border-neutral-200 bg-white px-2 text-[11px] text-neutral-600 hover:bg-neutral-50"
            @click="openSession"
          >
            <ExternalLink class="h-3 w-3" />
            查看会话
          </button>
          <button
            v-if="canDetach"
            class="flex h-6 cursor-pointer items-center gap-1 rounded-md border border-neutral-200 bg-white px-2 text-[11px] text-neutral-600 hover:bg-neutral-50 disabled:cursor-not-allowed disabled:opacity-40"
            :disabled="busy"
            title="解除任务与会话的绑定，下次启动会新建会话"
            @click="onDetach"
          >
            <Unlink class="h-3 w-3" />
            断开会话
          </button>
        </section>

        <!-- 工作树：登记着工作树 / 有清理失败记录 / 有已保留的分支 才显示 -->
        <section
          v-if="worktreePath || worktreeCleanupFailed || canDeleteBranch"
          class="mt-3 space-y-1 rounded-md border border-neutral-200 px-2.5 py-2"
        >
          <h3 class="text-[11px] font-medium text-neutral-500">工作树</h3>
          <div v-if="worktreePath" class="flex items-center gap-1.5">
            <span class="shrink-0 text-[11px] text-neutral-500">路径</span>
            <span
              class="min-w-0 flex-1 select-text truncate font-mono text-[11px] text-neutral-700"
              :title="worktreePath"
            >
              {{ worktreePath }}
            </span>
            <button
              class="shrink-0 cursor-pointer text-neutral-300 hover:text-neutral-500"
              title="复制路径"
              @click="copyValue('worktree', worktreePath)"
            >
              <Check v-if="copiedKey === 'worktree'" class="h-3 w-3 text-green-600" />
              <Copy v-else class="h-3 w-3" />
            </button>
          </div>
          <div v-if="task.workBranch || baseSummary" class="flex items-center gap-1.5">
            <span class="shrink-0 text-[11px] text-neutral-500">分支</span>
            <span
              class="min-w-0 flex-1 select-text truncate font-mono text-[11px] text-neutral-700"
              :title="task.workBranch ?? undefined"
            >
              {{ task.workBranch ?? '—' }}
            </span>
            <button
              v-if="task.workBranch"
              class="shrink-0 cursor-pointer text-neutral-300 hover:text-neutral-500"
              title="复制分支名"
              @click="copyValue('branch', task.workBranch)"
            >
              <Check v-if="copiedKey === 'branch'" class="h-3 w-3 text-green-600" />
              <Copy v-else class="h-3 w-3" />
            </button>
            <span v-if="baseSummary" class="shrink-0 text-[10px] text-neutral-400">
              基线 {{ baseSummary }}
            </span>
            <!-- 清理失败时路径还在：删除分支属于「重试清理」，走下方 amber 区，这里不重复 -->
          </div>
          <!-- 自动提交失败（task 10-26-worktree-autocommit / R4）：未提交的改动已随目录丢失 -->
          <div
            v-if="worktreeAutocommit === 'failed'"
            class="flex items-start gap-1.5 rounded border border-amber-200 bg-amber-50 px-2 py-1.5"
          >
            <AlertTriangle class="mt-0.5 h-3 w-3 shrink-0 text-amber-600" />
            <p class="min-w-0 flex-1 select-text break-words text-[11px] leading-4 text-amber-700">
              自动提交失败：<span class="font-mono">{{
                worktreeAutocommitReason || '未知原因'
              }}</span>
              （未提交的改动已丢失）。可先配置
              <span class="font-mono">git config --global user.name / user.email</span>
              再自行补救。
            </p>
          </div>
          <!-- 自动提交成功（静默路径，只有「终态 + 分支保留」才显示）：成果在分支上 -->
          <p
            v-else-if="worktreeAutocommit === 'ok'"
            class="text-[10px] leading-4 text-neutral-400"
          >
            已自动提交未保存改动到 <span class="select-text font-mono">{{ retainedBranch }}</span>
          </p>
          <!-- 分支已保留（keep-branch R3 / R4）：终态且目录已删 → 只剩分支这个产品 -->
          <div
            v-if="canDeleteBranch"
            class="flex items-start gap-1.5 rounded border border-blue-200 bg-blue-50 px-2 py-1.5"
          >
            <GitBranch class="mt-0.5 h-3 w-3 shrink-0 text-blue-600" />
            <div class="min-w-0 flex-1">
              <p class="text-[11px] leading-4 text-blue-800">
                分支
                <span class="select-text font-mono">{{ retainedBranch }}</span>
                已保留，可用 <span class="font-mono">git log</span> /
                <span class="font-mono">git diff</span> 查看，或
                <span class="font-mono">git merge</span> 合入其他分支。
              </p>
              <!-- 未提交改动是否还在取决于自动提交结果（task 10-26-worktree-autocommit）：
                   failed 已在上面 amber 区说过「已丢失」，这里只讲目录与分支的命运 -->
              <p class="text-[10px] leading-4 text-blue-700/80">
                工作树目录已删除，{{ worktreeAutocommit === 'failed' ? '未提交的改动已丢失，' : '' }}仅提交进分支的内容还在。
                删除分支后这些提交只能靠 <span class="font-mono">git fsck</span> 找回。
              </p>
            </div>
            <NPopconfirm @positive-click="onDeleteBranch">
              <template #trigger>
                <button
                  class="flex h-6 shrink-0 cursor-pointer items-center gap-1 rounded-md border border-blue-300 bg-white px-2 text-[11px] text-blue-700 hover:bg-blue-100 disabled:cursor-not-allowed disabled:opacity-40"
                  :disabled="busy"
                  title="永久删除这条已保留的分支（不可恢复）"
                >
                  <Trash2 class="h-3 w-3" />
                  删除分支
                </button>
              </template>
              <div class="max-w-64 text-[12px] leading-5">
                删除分支
                <span class="font-mono">{{ retainedBranch }}</span>
                ？<br />
                分支上<span class="font-medium">未合并的提交</span>会变成不可达提交（只能靠
                <span class="font-mono">git fsck</span>
                找回），此操作<span class="font-medium">不可撤销</span>。
              </div>
            </NPopconfirm>
          </div>
          <!-- 终态还挂着路径 = 清理失败（lifecycle R3 / AC9）：提示 + 路径 + 重试 -->
          <div
            v-if="worktreeCleanupFailed"
            class="flex items-center gap-1.5 rounded border border-amber-200 bg-amber-50 px-2 py-1.5"
          >
            <AlertTriangle class="h-3 w-3 shrink-0 text-amber-600" />
            <div class="min-w-0 flex-1">
              <p class="text-[11px] leading-4 text-amber-700">工作树清理失败</p>
              <p
                v-if="worktreePath"
                class="select-text break-all font-mono text-[10px] leading-4 text-amber-600/80"
              >
                {{ worktreePath }}
              </p>
              <p v-else class="text-[10px] leading-4 text-amber-600/80">
                目录已删除，清理记录未写回；重试可清掉这条记录
              </p>
            </div>
            <button
              class="flex h-6 shrink-0 cursor-pointer items-center gap-1 rounded-md border border-amber-300 bg-white px-2 text-[11px] text-amber-700 hover:bg-amber-100 disabled:cursor-not-allowed disabled:opacity-40"
              :disabled="busy"
              title="重新尝试删除工作树目录"
              @click="onRetryWorktreeCleanup"
            >
              <RotateCcw class="h-3 w-3" />
              重试清理
            </button>
          </div>
        </section>

        <!-- 时间线（升序） -->
        <section class="mt-4">
          <h3 class="mb-1.5 flex items-center gap-1 text-[11px] font-medium text-neutral-500">
            <History class="h-3 w-3" />
            时间线
          </h3>
          <p v-if="loadingEvents" class="text-[11px] text-neutral-400">读取中…</p>
          <p v-else-if="events.length === 0" class="text-[11px] text-neutral-400">暂无记录</p>
          <ol v-else class="space-y-1">
            <li v-for="event in events" :key="event.id" class="flex items-baseline gap-2 text-[11px]">
              <span class="w-[104px] shrink-0 font-mono text-[10px] text-neutral-400">
                {{ formatDateTime(event.createdAt) }}
              </span>
              <span v-if="statusParts(event)" class="min-w-0 flex-1 text-neutral-700">
                {{ statusParts(event)?.from }}
                <span class="px-0.5 text-neutral-300">→</span>
                {{ statusParts(event)?.to }}
              </span>
              <span v-else class="min-w-0 flex-1 text-neutral-700">{{ eventLabel(event) }}</span>
              <span class="shrink-0 text-[10px] text-neutral-400">{{ actorLabel(event) }}</span>
            </li>
          </ol>
        </section>
      </div>

      <!-- 动作区：按状态给全量动作 -->
      <div class="flex flex-wrap items-center gap-1.5 border-t border-neutral-200 px-4 py-2.5">
        <button
          v-if="editable"
          class="flex h-7 cursor-pointer items-center gap-1 rounded-md border border-neutral-200 bg-white px-2.5 text-[12px] text-neutral-600 hover:bg-neutral-50 disabled:cursor-not-allowed disabled:opacity-40"
          :disabled="busy"
          @click="onEdit"
        >
          <Pencil class="h-3 w-3" />
          编辑
        </button>

        <button
          v-if="task.status === 'todo'"
          class="flex h-7 cursor-pointer items-center gap-1 rounded-md border border-neutral-200 bg-white px-2.5 text-[12px] text-neutral-600 hover:bg-neutral-50 disabled:cursor-not-allowed disabled:opacity-40"
          :disabled="busy"
          @click="onStart"
        >
          <Play class="h-3 w-3" />
          启动
        </button>

        <button
          v-if="task.status === 'failed'"
          class="flex h-7 cursor-pointer items-center gap-1 rounded-md border border-neutral-200 bg-white px-2.5 text-[12px] text-neutral-600 hover:bg-neutral-50 disabled:cursor-not-allowed disabled:opacity-40"
          :disabled="busy"
          @click="onRetry"
        >
          <RotateCcw class="h-3 w-3" />
          重试
        </button>

        <button
          v-if="running"
          class="flex h-7 cursor-pointer items-center gap-1 rounded-md border border-neutral-200 bg-white px-2.5 text-[12px] text-neutral-600 hover:bg-neutral-50 disabled:cursor-not-allowed disabled:opacity-40"
          :disabled="busy"
          @click="onStop"
        >
          <Square class="h-2.5 w-2.5" />
          停止
        </button>

        <button
          v-if="task.status === 'review'"
          class="flex h-7 cursor-pointer items-center gap-1 rounded-md bg-blue-500 px-2.5 text-[12px] text-white hover:bg-blue-600 disabled:cursor-not-allowed disabled:opacity-40"
          :disabled="busy"
          @click="onApprove"
        >
          <Check class="h-3 w-3" />
          通过
        </button>
        <button
          v-if="task.status === 'review'"
          class="flex h-7 cursor-pointer items-center gap-1 rounded-md border border-neutral-200 bg-white px-2.5 text-[12px] text-neutral-600 hover:bg-neutral-50 disabled:cursor-not-allowed disabled:opacity-40"
          :disabled="busy"
          @click="onReject"
        >
          <CornerDownLeft class="h-3 w-3" />
          打回
        </button>

        <span class="flex-1" />

        <!-- 浏览器 dev：手动模拟 turn 结论（Tauri 不显示） -->
        <template v-if="devMode && canSimulate">
          <button
            class="flex h-7 cursor-pointer items-center gap-1 rounded-md border border-dashed border-neutral-300 px-2 text-[11px] text-neutral-500 hover:bg-neutral-50"
            title="仅浏览器调试：模拟 Agent 正常结束"
            @click="simulate('ok')"
          >
            模拟完成
          </button>
          <button
            class="flex h-7 cursor-pointer items-center gap-1 rounded-md border border-dashed border-neutral-300 px-2 text-[11px] text-neutral-500 hover:bg-neutral-50"
            title="仅浏览器调试：模拟用户停止（任务保持进行中）"
            @click="simulate('stop')"
          >
            模拟停止
          </button>
          <button
            class="flex h-7 cursor-pointer items-center gap-1 rounded-md border border-dashed border-neutral-300 px-2 text-[11px] text-neutral-500 hover:bg-neutral-50"
            title="仅浏览器调试：模拟 Agent 空回复"
            @click="simulate('empty')"
          >
            模拟失败
          </button>
        </template>

        <button
          class="flex h-7 cursor-pointer items-center gap-1 rounded-md border border-neutral-200 bg-white px-2.5 text-[12px] text-neutral-600 hover:bg-neutral-50 disabled:cursor-not-allowed disabled:opacity-40"
          :disabled="busy || !task.prompt.trim()"
          title="用同样的项目 / 提示词 / 引擎再建一个任务"
          @click="onDuplicate"
        >
          <Copy class="h-3 w-3" />
          复制
        </button>
        <button
          v-if="archivable"
          class="flex h-7 cursor-pointer items-center gap-1 rounded-md border border-neutral-200 bg-white px-2.5 text-[12px] text-neutral-600 hover:bg-neutral-50 disabled:cursor-not-allowed disabled:opacity-40"
          :disabled="busy"
          @click="onArchive"
        >
          <Archive class="h-3 w-3" />
          {{ task.archivedAt === null ? '归档' : '取消归档' }}
        </button>
        <button
          class="flex h-7 cursor-pointer items-center gap-1 rounded-md border border-neutral-200 bg-white px-2.5 text-[12px] text-red-600 hover:bg-red-50 disabled:cursor-not-allowed disabled:opacity-40"
          :disabled="busy"
          @click="onDelete"
        >
          <Trash2 class="h-3 w-3" />
          删除
        </button>
        <LoaderCircle v-if="busy" class="h-3.5 w-3.5 animate-spin text-blue-500" />
      </div>
    </div>
  </div>
</template>
