// 任务看板域模型（task 10-17-task-data-layer）：唯一类型来源，Rust 侧
// src-tauri/src/task.rs 的 #[serde(rename_all = "camelCase")] 字段与之逐字对齐。
// 状态转移的唯一权威在 Rust（TRANSITIONS）；本文件只描述形状与展示映射。

import type { EngineWire } from './engine'

/**
 * 任务状态（含 codeg 兼容的 merging：本任务不产生，仅保留枚举以对齐列映射）。
 * todo / queued 归「待办」，preparing / running 归「进行中」，
 * awaiting_input / review / merging / failed 归「等你处理」，done / canceled 归「已完成」。
 */
export type TaskStatus =
  | 'todo'
  | 'queued'
  | 'preparing'
  | 'running'
  | 'awaiting_input'
  | 'review'
  | 'merging'
  | 'done'
  | 'failed'
  | 'canceled'

/** 看板四列（固定 4 列，PRD D2） */
export type TaskColumnId = 'todo' | 'inProgress' | 'attention' | 'done'

/** 审核结论（agent 侧回填；本数据层只存形状） */
export type TaskVerdict = 'success' | 'needs_review' | 'blocked'

/** 失败归因：agent 报错 / 被中断 / 空回复 */
export type TaskFailureReason = 'agent_error' | 'interrupted' | 'empty_turn'

export interface WorkTask {
  /** 'task-xxxxxxxx' */
  id: string
  /** 所属 Project.id（'proj-xxxx'）；项目被移除后仍保留（prd PAC4） */
  projectId: string
  title: string
  /** 发给 Agent 的正文（首版纯文本） */
  prompt: string
  status: TaskStatus
  /** 列内排序，仅同 projectId 内有意义 */
  sortOrder: number
  /** 每次启动 +1；用于丢弃迟到的 turn 回调 */
  runSeq: number
  /** 一对一 ACP 会话（本地 session.id） */
  sessionId: string | null
  /** 创建时固化的引擎 wire 标识，启动时用它建会话 */
  engineId: EngineWire | null
  /** 编辑器里选的模型 id（ACP config option 值）；null = 引擎默认模型 */
  modelId: string | null
  /** 思考程度的 config option id（如 `effort`）与取值；引擎不支持时均为 null */
  effortId: string | null
  effortValue: string | null
  verdict: TaskVerdict | null
  resultSummary: string | null
  lastError: string | null
  failureReason: TaskFailureReason | null
  archivedAt: number | null
  createdAt: number
  updatedAt: number
  startedAt: number | null
  /** 进入 review / failed 的时间 */
  settledAt: number | null
  /** 进入 done / canceled 的时间 */
  finishedAt: number | null
}

export type TaskEventKind =
  | 'created'
  | 'status_changed'
  | 'user_action'
  | 'agent_progress'
  | 'agent_verdict'

export type TaskActor = 'user' | 'engine' | 'agent'

/** 任务时间线事件（append-only，每任务保留最近 200 条） */
export interface WorkTaskEvent {
  id: string
  taskId: string
  kind: TaskEventKind
  actor: TaskActor
  payload: Record<string, unknown> | null
  createdAt: number
}

/** 新建任务入参（task_create） */
export interface TaskDraft {
  projectId: string
  title: string
  /**
   * 省略即空正文。显式 `undefined` 在 JSON 序列化时会被丢弃，
   * 因此 Rust 侧 `#[serde(default)]` 能正确落到空串。
   */
  prompt?: string
  /** 省略 / null 则在启动时才决定引擎 */
  engineId?: EngineWire | null
  /** 省略 / null = 用引擎默认模型 */
  modelId?: string | null
  /** 思考程度：config option 的 id 与取值（引擎不支持时省略） */
  effortId?: string | null
  effortValue?: string | null
}

/**
 * 编辑入参（task_update）：未提供的字段不改（对应 Rust `TaskPatch` 的 `Option` 字段）。
 * 语义是「缺省 = 不改」而非「null = 清空」——当前无清空 `engineId` 的接口；
 * 将来要支持时两侧需同时改。`modelId` 传空串表示回到引擎默认。
 */
export interface TaskPatch {
  title?: string
  prompt?: string
  engineId?: EngineWire | null
  modelId?: string | null
  effortId?: string | null
  effortValue?: string | null
}

/** 一个任务的完整视图：行 + 时间线（Rust `TaskDetails`） */
export interface TaskDetails {
  task: WorkTask
  events: WorkTaskEvent[]
}

/**
 * turn 回流结论（`task_settle_turn` 的 outcome，子任务 3 / prd R5）。
 * 由前端从 `acp_prompt` 的 stopReason + 错误分类得出；Rust 只做
 * 「runSeq 防串 + 状态机校验 + 写 failureReason」。
 */
export type TaskTurnOutcome = 'ok' | 'error' | 'empty' | 'cancelled'

/**
 * task 命令约定错误串：`not-found` / `stale` / `illegal-transition: <from>-><to>` / `io: <msg>`。
 * 注意 `io:` 段之后是**具体原因**；Rust 侧部分业务拒绝（编辑/归档）也用 `io:` 前缀，
 * 因为 `Result<_, String>` 里只有它约定能带消息。
 */
export type TaskErrorCode = 'not-found' | 'stale' | 'illegal-transition' | 'io'

export const TASK_ERROR_MESSAGES: Record<TaskErrorCode, string> = {
  'not-found': '任务不存在',
  stale: '任务状态已被其他操作改变，请刷新后重试',
  'illegal-transition': '当前状态不允许该操作',
  io: '任务数据读写失败',
}

/** 把 Rust 错误串归类；非约定错误返回 null（调用方显示原文） */
export function classifyTaskError(err: unknown): TaskErrorCode | null {
  const msg = err instanceof Error ? err.message : String(err ?? '')
  // 顺序有意义：先匹配更具体的码，避免被消息里的普通词误判
  if (msg.includes('illegal-transition')) return 'illegal-transition'
  if (msg.includes('not-found')) return 'not-found'
  if (msg.includes('stale')) return 'stale'
  if (msg.includes('io:')) return 'io'
  return null
}

/**
 * 错误串 → 用户可读中文提示。状态转移失败带出 from->to 细节，
 * 便于用户理解「为什么这一列拖不过去」。
 */
export function taskErrorMessage(err: unknown): string {
  const msg = err instanceof Error ? err.message : String(err ?? '')
  const code = classifyTaskError(err)
  if (code === 'illegal-transition') {
    const detail = msg.split('illegal-transition:')[1]?.trim()
    const [from, to] = (detail ?? '').split('->').map((s) => s.trim())
    if (from && to) return `不允许从「${TASK_STATUS_LABELS[from as TaskStatus] ?? from}」变为「${TASK_STATUS_LABELS[to as TaskStatus] ?? to}」`
    return TASK_ERROR_MESSAGES['illegal-transition']
  }
  if (code === 'io') {
    // io 段之后是具体原因，原样展示更有用
    const detail = msg.split('io:')[1]?.trim()
    return detail || TASK_ERROR_MESSAGES.io
  }
  if (code) return TASK_ERROR_MESSAGES[code]
  return msg || '任务操作失败'
}

/** 状态中文名（chip / 时间线用） */
export const TASK_STATUS_LABELS: Record<TaskStatus, string> = {
  todo: '待办',
  queued: '排队中',
  preparing: '准备中',
  running: '进行中',
  awaiting_input: '等待输入',
  review: '待审核',
  merging: '合并中',
  done: '已完成',
  failed: '失败',
  canceled: '已取消',
}

/** 任务编辑态里可改字段的状态（Rust `task_update` 同样只放行这两个） */
export function isTaskEditable(status: TaskStatus): boolean {
  return status === 'todo' || status === 'failed'
}
