// 自动化域模型（task 10-26-automation-scheduler）：与 Rust
// src-tauri/src/automation/store.rs 的 #[serde(rename_all = "camelCase")] 逐字段对齐。
//
// 时间是 i64 UTC 毫秒（number | null），与其他域模型一致。

import type { EngineWire } from './engine'

/** 触发方式：按 cron 定时 / 只能手动 */
export type TriggerKind = 'schedule' | 'manual'

/** 触发后做什么：起无头会话跑完 / 往看板塞一张 todo 卡 */
export type AutomationAction = 'launch_session' | 'enqueue_task'

/** 一次运行的生命周期 */
export type AutomationRunStatus =
  | 'running'
  | 'succeeded'
  | 'failed'
  | 'cancelled'
  /** 上一条还没跑完就又到点，被抑制掉的那次 */
  | 'skipped'

/** 一条自动化（对应 Rust `Automation`） */
export interface Automation {
  /** 'auto-xxxxxxxx' */
  id: string
  name: string
  enabled: boolean
  triggerKind: TriggerKind
  /** 5 字段 cron（分 时 日 月 周）；周字段 0/7=周日 */
  cron: string
  /** IANA 时区名（如 Asia/Shanghai） */
  timezone: string
  /** 下次触发时刻（UTC 毫秒）；null = 不再触发 */
  nextRunAt: number | null
  engineId: EngineWire
  projectId: string
  /** 项目根目录快照（触发前校验存在，不在触发时反查项目表） */
  rootPath: string
  modelId: string | null
  effortId: string | null
  effortValue: string | null
  prompt: string
  action: AutomationAction
  /** 建出的任务是否使用独立工作树 */
  useWorktree: boolean
  baseBranch: string | null
  initCommand: string | null
  lastRunAt: number | null
  lastRunStatus: AutomationRunStatus | null
  lastRunId: string | null
  /** 用户还没看过的失败次数（侧栏徽标） */
  unseenFailures: number
  createdAt: number
  updatedAt: number
  deletedAt: number | null
}

/** 新建 / 编辑的完整草稿（编辑器整条保存，无局部补丁） */
export interface AutomationDraft {
  name: string
  enabled: boolean
  triggerKind: TriggerKind
  cron: string
  timezone: string
  engineId: EngineWire
  projectId: string
  rootPath: string
  modelId: string | null
  effortId: string | null
  effortValue: string | null
  prompt: string
  action: AutomationAction
  useWorktree: boolean
  baseBranch: string | null
  initCommand: string | null
}

/** 一条运行记录 */
export interface AutomationRun {
  /** 'arun-xxxxxxxx' */
  id: string
  automationId: string
  status: AutomationRunStatus
  trigger: 'schedule' | 'manual'
  /** 本次消费的时间片（UTC 毫秒）；manual 为 null */
  scheduledFor: number | null
  startedAt: number | null
  endedAt: number | null
  /** launch_session 的会话 id（可用于回放） */
  sessionId: string | null
  /** enqueue_task 建出的任务 id */
  taskId: string | null
  stopReason: string | null
  error: string | null
  summary: string | null
  createdAt: number
}

/** `automation_compute_next_run` 的返回（编辑器预览） */
export interface NextRunPreview {
  nextRunAt: number | null
  /** 该时区是否在支持表内（表外按 UTC 求值） */
  timezoneSupported: boolean
  /** 该时区是否有夏令时（本实现按标准时偏移求值） */
  timezoneHasDst: boolean
}

/** 动作的中文标签（单一来源，卡片与编辑器共用） */
export const ACTION_LABELS: Record<AutomationAction, string> = {
  launch_session: '启动无头会话',
  enqueue_task: '加入任务看板',
}

/** 动作的一句话说明 */
export const ACTION_HINTS: Record<AutomationAction, string> = {
  launch_session: '直接拉起一个后台会话把提示词跑完，结果记在运行历史里（会自动允许工具调用）',
  enqueue_task: '只在看板上建一张待办卡，由你自己决定什么时候启动它',
}

/** 运行状态的中文标签 */
export const RUN_STATUS_LABELS: Record<AutomationRunStatus, string> = {
  running: '运行中',
  succeeded: '成功',
  failed: '失败',
  cancelled: '已取消',
  skipped: '已跳过',
}

/** 运行状态色系（naive-ui tag type 对应值） */
export const RUN_STATUS_TYPES: Record<
  AutomationRunStatus,
  'default' | 'success' | 'error' | 'warning' | 'info'
> = {
  running: 'info',
  succeeded: 'success',
  failed: 'error',
  cancelled: 'default',
  skipped: 'warning',
}

/**
 * 触发器类型是否合法（草稿阶段的最小校验；权威校验在 Rust `validate_draft`）。
 */
export function isAutomationAction(value: string): value is AutomationAction {
  return value === 'launch_session' || value === 'enqueue_task'
}

/**
 * 错误串 → 可读中文。
 *
 * Rust 侧错误串沿用仓库词汇（`not-found` / `io: ...` /
 * `automation engine not running` / `previous run still active`），
 * 这里统一翻译一次，组件不再各自匹配字符串。
 */
export function automationErrorMessage(err: unknown): string {
  const raw = err instanceof Error ? err.message : String(err ?? '')
  if (!raw) return '操作失败'
  if (raw.includes('automation engine not running')) {
    return '自动化调度未在当前进程运行（可能已开另一个实例），只能编辑不能触发'
  }
  if (raw.includes('previous run still active')) {
    return '上一条运行还没结束，本次已跳过'
  }
  if (raw.startsWith('not-found')) {
    return '自动化不存在或已被删除'
  }
  if (raw.startsWith('io: ')) {
    return raw.slice(4)
  }
  return raw
}
