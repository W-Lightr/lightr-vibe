// 浏览器 dev 环境的任务 mock（PRD R5）：内存实现 + 同一状态机。
// 契约与 Rust 命令逐一对齐（错误串、返回形状、时间戳副作用），
// 组件 / store 经 taskService 统一消费，不感知通道差异。
// 浏览器下无法持久化（刷新即重置），但演示任务保证看板有内容可看（AC7）。

import type {
  TaskActor,
  TaskDetails,
  TaskDraft,
  TaskEventKind,
  TaskPatch,
  TaskStatus,
  WorkTask,
  WorkTaskEvent,
} from '../types/task'
import type { EngineWire } from '../types/engine'

/** 每任务事件上限（与 Rust MAX_EVENTS_PER_TASK 一致） */
const MAX_EVENTS_PER_TASK = 200

/** 状态转换表（与 Rust TRANSITIONS 逐条对齐，design §3.1） */
const TRANSITIONS: Record<TaskStatus, TaskStatus[]> = {
  todo: ['queued', 'canceled'],
  queued: ['preparing', 'todo', 'canceled'],
  preparing: ['running', 'failed', 'canceled'],
  running: ['awaiting_input', 'review', 'failed', 'canceled'],
  awaiting_input: ['running', 'canceled'],
  review: ['done', 'todo', 'canceled'],
  failed: ['queued', 'canceled'],
  canceled: ['todo'],
  merging: [],
  done: [],
}

/** 纯函数：from 是否允许转移到 to（含自环，UI 同列重排用） */
export function canTransition(from: TaskStatus, to: TaskStatus): boolean {
  if (from === to) return true
  return TRANSITIONS[from].includes(to)
}

const TERMINAL: TaskStatus[] = ['done', 'canceled', 'failed']

/** mock 侧 ID：`<prefix>-xxxxxxxx`（与 Rust create_id 的 8 位十六进制后缀同形） */
function createMockId(prefix: string): string {
  const c = globalThis.crypto
  const suffix =
    c && typeof c.randomUUID === 'function'
      ? c.randomUUID().replace(/-/g, '').slice(0, 8)
      : Math.random().toString(16).slice(2, 10).padEnd(8, '0')
  return `${prefix}-${suffix}`
}

function nowMs(): number {
  return Date.now()
}

// ---------- 内存态 ----------

let tasks: WorkTask[] = []
let events: WorkTaskEvent[] = []
let seeded = false

function projectA(): string {
  return 'proj-29028'
}

/** 3 条演示任务（浏览器 dev 下看板非空；一个项目内三种列状态） */
function seedDemoTasks(): void {
  if (seeded) return
  seeded = true
  const now = nowMs()
  // 注意：状态只能通过 `applyEntryEffects` + 状态机「走」出来，不能直接写
  //（否则 mock 会出现 Rust 永远产生不了的状态组合，比如 review 却 runSeq=0）。
  const demo: { title: string; prompt: string; path: TaskStatus[]; offset: number }[] = [
    {
      title: '修复登录页在窄屏下的布局错位',
      prompt: '登录页在 375px 宽度下按钮溢出，请修复并补充测试。',
      path: [],
      offset: 25,
    },
    {
      title: '为会话列表补充分页',
      prompt: '会话列表超过 100 条时渲染变慢，实现分页或虚拟滚动。',
      path: ['queued', 'preparing', 'running'],
      offset: 12,
    },
    {
      title: '整理 README 的快速开始一节',
      prompt: '按当前命令重写 README 的快速开始，确保新用户能一次跑通。',
      path: ['queued', 'preparing', 'running', 'review'],
      offset: 3,
    },
  ]
  demo.forEach((item, index) => {
    const created = now - item.offset * 3_600_000
    const task: WorkTask = {
      id: createMockId('task'),
      projectId: projectA(),
      title: item.title,
      prompt: item.prompt,
      status: 'todo',
      sortOrder: index,
      runSeq: 0,
      sessionId: null,
      engineId: 'open_code',
      verdict: null,
      resultSummary: null,
      lastError: null,
      failureReason: null,
      archivedAt: null,
      createdAt: created,
      updatedAt: now - item.offset * 60_000,
      startedAt: null,
      settledAt: null,
      finishedAt: null,
    }
    tasks.push(task)
    pushEvent(task.id, 'created', 'user', null)
    const lastStep = item.path.length
    item.path.forEach((to, step) => {
      const from = task.status
      const at = created + Math.round(((step + 1) / (lastStep + 1)) * item.offset * 60_000)
      task.status = to
      applyEntryEffects(task, to, at)
      pushEvent(task.id, 'status_changed', 'user', { from, to })
    })
    if (item.path.length > 0) {
      // 演示数据里「已启动」的任务带一个会话 id（真实链路由 attach_session 写入）
      task.sessionId = createMockId('ses')
    }
  })
}

function ensureSeed(): void {
  seedDemoTasks()
}

// ---------- 内部工具 ----------

function pushEvent(
  taskId: string,
  kind: TaskEventKind,
  actor: TaskActor,
  payload: Record<string, unknown> | null,
): void {
  events.push({
    id: createMockId('evt'),
    taskId,
    kind,
    actor,
    payload,
    createdAt: nowMs(),
  })
  // 每任务截断到最近 200 条（与 Rust 一致）
  const own = events.filter((e) => e.taskId === taskId)
  if (own.length > MAX_EVENTS_PER_TASK) {
    const drop = new Set(own.slice(0, own.length - MAX_EVENTS_PER_TASK).map((e) => e.id))
    events = events.filter((e) => !drop.has(e.id))
  }
}

function findIndex(id: string): number {
  const index = tasks.findIndex((t) => t.id === id)
  if (index === -1) throw new Error('not-found')
  return index
}

function clone(task: WorkTask): WorkTask {
  return { ...task }
}

function eventsOf(taskId: string): WorkTaskEvent[] {
  return events
    .filter((e) => e.taskId === taskId)
    .sort((a, b) => a.createdAt - b.createdAt)
    .map((e) => ({ ...e }))
}

/** 时间戳副作用（与 Rust apply_entry_effects 对齐，prd R4） */
function applyEntryEffects(task: WorkTask, to: TaskStatus, now: number): void {
  if (to === 'queued') {
    task.runSeq += 1
    task.startedAt ??= now
  }
  if (to === 'review' || to === 'failed') task.settledAt = now
  if (to === 'done' || to === 'canceled') task.finishedAt = now
}

function transitionInner(id: string, to: TaskStatus, expectedFrom: TaskStatus | null): WorkTask {
  const index = findIndex(id)
  const task = tasks[index]
  const from = task.status
  if (expectedFrom && expectedFrom !== from) throw new Error('stale')
  if (!canTransition(from, to)) throw new Error(`illegal-transition: ${from}->${to}`)
  if (from === to) return clone(task)
  const now = nowMs()
  task.status = to
  task.updatedAt = now
  applyEntryEffects(task, to, now)
  pushEvent(id, 'status_changed', 'user', { from, to })
  return clone(task)
}

// ---------- mock 命令（形状对齐 task_* 命令） ----------

export function mockList(projectId?: string): WorkTask[] {
  ensureSeed()
  const rows = projectId ? tasks.filter((t) => t.projectId === projectId) : [...tasks]
  if (projectId) return rows.sort((a, b) => a.sortOrder - b.sortOrder).map(clone)
  return rows.sort((a, b) => b.updatedAt - a.updatedAt).map(clone)
}

export function mockCreate(draft: TaskDraft): TaskDetails {
  ensureSeed()
  const title = draft.title.trim()
  if (!title) throw new Error('io: 任务标题不能为空')
  const now = nowMs()
  const sortOrder =
    tasks.filter((t) => t.projectId === draft.projectId).reduce((max, t) => Math.max(max, t.sortOrder), -1) + 1
  const task: WorkTask = {
    id: createMockId('task'),
    projectId: draft.projectId,
    title,
    prompt: draft.prompt ?? '',
    status: 'todo',
    sortOrder,
    runSeq: 0,
    sessionId: null,
    engineId: draft.engineId ?? null,
    verdict: null,
    resultSummary: null,
    lastError: null,
    failureReason: null,
    archivedAt: null,
    createdAt: now,
    updatedAt: now,
    startedAt: null,
    settledAt: null,
    finishedAt: null,
  }
  tasks.push(task)
  pushEvent(task.id, 'created', 'user', null)
  return { task: clone(task), events: eventsOf(task.id) }
}

export function mockUpdate(id: string, patch: TaskPatch): WorkTask {
  ensureSeed()
  const task = tasks[findIndex(id)]
  if (task.status !== 'todo' && task.status !== 'failed') {
    throw new Error(`io: 当前状态 ${task.status} 不允许编辑任务`)
  }
  if (patch.title !== undefined) {
    const title = patch.title.trim()
    if (!title) throw new Error('io: 任务标题不能为空')
    task.title = title
  }
  if (patch.prompt !== undefined) task.prompt = patch.prompt
  if (patch.engineId !== undefined) task.engineId = patch.engineId
  task.updatedAt = nowMs()
  pushEvent(id, 'user_action', 'user', { action: 'updated' })
  return clone(task)
}

export function mockDelete(id: string): void {
  ensureSeed()
  const index = findIndex(id)
  tasks.splice(index, 1)
  events = events.filter((e) => e.taskId !== id)
}

export function mockTransition(
  id: string,
  to: TaskStatus,
  expectedFrom: TaskStatus | null,
): WorkTask {
  ensureSeed()
  return transitionInner(id, to, expectedFrom)
}

export function mockReorder(projectId: string, orderedIds: string[]): WorkTask[] {
  ensureSeed()
  const now = nowMs()
  orderedIds.forEach((id, index) => {
    const task = tasks.find((t) => t.id === id && t.projectId === projectId)
    if (!task) return // 别的项目的 id 静默忽略（作用域过滤，AC4）
    task.sortOrder = index
    task.updatedAt = now
  })
  return tasks.filter((t) => t.projectId === projectId).map(clone)
}

export function mockAttachSession(
  id: string,
  sessionId: string,
  engineId: EngineWire | null,
): WorkTask {
  ensureSeed()
  const task = tasks[findIndex(id)]
  task.sessionId = sessionId
  if (engineId !== null) task.engineId = engineId
  task.updatedAt = nowMs()
  pushEvent(id, 'user_action', 'user', { action: 'attach_session', sessionId })
  return clone(task)
}

export function mockArchive(id: string, archived: boolean): WorkTask {
  ensureSeed()
  const task = tasks[findIndex(id)]
  if (archived && !TERMINAL.includes(task.status)) {
    throw new Error(`io: 任务状态 ${task.status} 不可归档`)
  }
  task.archivedAt = archived ? nowMs() : null
  task.updatedAt = nowMs()
  pushEvent(id, 'user_action', 'user', { action: archived ? 'archive' : 'unarchive' })
  return clone(task)
}

export function mockEvents(id: string): WorkTaskEvent[] {
  ensureSeed()
  return eventsOf(id)
}
