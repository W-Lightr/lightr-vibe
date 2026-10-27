// 浏览器 dev 环境的自动化 mock：内存实现 + 与 Rust 命令同形的返回值。
//
// **重要边界**：这里的 cron 求值只求覆盖 UI 演示路径（每分钟 / 每天 N 点 /
// 每周 N / 每月 1 号），**不追求与 Rust 等价** —— 浏览器里没有真调度器，
// 也不会自动触发。真机行为以 Rust 的 `automation/cron.rs` 为准。
//
// 组件 / store 只经 automationService 消费，不感知通道差异。

import type {
  Automation,
  AutomationDraft,
  AutomationRun,
  NextRunPreview,
  TriggerKind,
} from '../types/automation'
import { createId } from '../utils/id'

let automations: Automation[] = []
let runs: AutomationRun[] = []
let seeded = false

const MINUTE = 60_000
const DAY = 86_400_000

/** 与 Rust 侧一致的默认时区（主要用户无 DST） */
const DEFAULT_TZ = 'Asia/Shanghai'

function now(): number {
  return Date.now()
}

/** 时区偏移（分钟）——mock 只认常用几个，其余按 0 */
function tzOffset(tz: string): number {
  const table: Record<string, number> = {
    UTC: 0,
    'Etc/UTC': 0,
    'Asia/Shanghai': 480,
    'Asia/Tokyo': 540,
    'Asia/Kolkata': 330,
    'Europe/London': 0,
    'Europe/Berlin': 60,
    'America/New_York': -300,
    'America/Los_Angeles': -480,
  }
  return table[tz] ?? 0
}

/**
 * 极简 cron 求值：只支持
 * - `* * * * *`（每分钟）
 * - `M H * * *`（每天 H:M）
 * - `M H * * D`（每周 D，0/7=周日）
 * - `M H D * *`（每月 D 号）
 * 其他返回 null（UI 上显示「该表达式不会再触发」）。
 */
function nextRunSimple(cron: string, timezone: string): number | null {
  const fields = cron.trim().split(/\s+/)
  if (fields.length !== 5) return null
  const [minRaw, hourRaw, domRaw, monRaw, dowRaw] = fields
  const num = (raw: string): number | null => (/^\d+$/.test(raw) ? Number(raw) : null)

  const min = minRaw === '*' ? 0 : num(minRaw)
  const hour = hourRaw === '*' ? 0 : num(hourRaw)
  if (min === null || hour === null || min > 59 || hour > 23) return null
  if (monRaw !== '*') return null

  const offset = tzOffset(timezone) * MINUTE
  const base = new Date(now() + offset)

  if (minRaw === '*' && hourRaw === '*' && domRaw === '*' && dowRaw === '*') {
    // 下一分钟
    return Math.floor((now() + offset) / MINUTE) * MINUTE + MINUTE - offset
  }

  const dom = domRaw === '*' ? null : num(domRaw)
  const dow = dowRaw === '*' ? null : num(dowRaw)
  if (dom === null && dow === null) {
    // 每天 H:M
    const target = new Date(base)
    target.setHours(hour, min, 0, 0)
    if (target.getTime() <= now() + offset) target.setTime(target.getTime() + DAY)
    return target.getTime() - offset
  }
  if (dow !== null) {
    const want = dow === 7 ? 0 : dow
    if (want > 6) return null
    const target = new Date(base)
    target.setHours(hour, min, 0, 0)
    let guard = 0
    while (target.getDay() !== want && guard < 8) {
      target.setTime(target.getTime() + DAY)
      guard += 1
    }
    if (target.getTime() <= now() + offset) target.setTime(target.getTime() + 7 * DAY)
    return target.getTime() - offset
  }
  // 每月 D 号
  const target = new Date(base)
  target.setDate(dom as number)
  target.setHours(hour, min, 0, 0)
  if (target.getTime() <= now() + offset || target.getDate() !== dom) {
    target.setMonth(target.getMonth() + 1, dom as number)
    target.setHours(hour, min, 0, 0)
    if (target.getDate() !== dom) return null
  }
  return target.getTime() - offset
}

function draftToAutomation(draft: AutomationDraft, id: string, createdAt: number): Automation {
  const nextRunAt =
    draft.enabled && draft.triggerKind === 'schedule'
      ? nextRunSimple(draft.cron, draft.timezone)
      : null
  return {
    id,
    name: draft.name.trim(),
    enabled: draft.enabled,
    triggerKind: draft.triggerKind,
    cron: draft.cron.trim(),
    timezone: draft.timezone.trim(),
    nextRunAt,
    engineId: draft.engineId,
    projectId: draft.projectId,
    rootPath: draft.rootPath,
    modelId: draft.modelId,
    effortId: draft.effortId,
    effortValue: draft.effortValue,
    prompt: draft.prompt,
    action: draft.action,
    useWorktree: draft.useWorktree,
    baseBranch: draft.baseBranch,
    initCommand: draft.initCommand,
    lastRunAt: null,
    lastRunStatus: null,
    lastRunId: null,
    unseenFailures: 0,
    createdAt,
    updatedAt: createdAt,
    deletedAt: null,
  }
}

/** 演示数据：让首次打开页面就有内容可看（对应 taskMock 的 seed 思路） */
function seedOnce(): void {
  if (seeded) return
  seeded = true
  const t = now()
  automations = [
    {
      id: 'auto-demo0001',
      name: '每日依赖巡检',
      enabled: true,
      triggerKind: 'schedule',
      cron: '0 9 * * *',
      timezone: DEFAULT_TZ,
      nextRunAt: nextRunSimple('0 9 * * *', DEFAULT_TZ),
      engineId: 'open_code',
      projectId: 'proj-demo',
      rootPath: 'D:\\demo\\lightr-vibe',
      modelId: null,
      effortId: null,
      effortValue: null,
      prompt: '检查依赖过期情况，汇总成一份简报。',
      action: 'enqueue_task',
      useWorktree: true,
      baseBranch: null,
      initCommand: null,
      lastRunAt: t - 3_600_000,
      lastRunStatus: 'succeeded',
      lastRunId: 'arun-demo0001',
      unseenFailures: 0,
      createdAt: t - 7 * DAY,
      updatedAt: t - 3_600_000,
      deletedAt: null,
    },
    {
      id: 'auto-demo0002',
      name: '每周一清理构建产物',
      enabled: false,
      triggerKind: 'schedule',
      cron: '0 10 * * 1',
      timezone: DEFAULT_TZ,
      nextRunAt: null,
      engineId: 'open_code',
      projectId: 'proj-demo',
      rootPath: 'D:\\demo\\lightr-vibe',
      modelId: null,
      effortId: null,
      effortValue: null,
      prompt: '删除过期的构建产物并汇报释放的空间。',
      action: 'launch_session',
      useWorktree: false,
      baseBranch: null,
      initCommand: null,
      lastRunAt: t - 2 * DAY,
      lastRunStatus: 'failed',
      lastRunId: 'arun-demo0002',
      unseenFailures: 1,
      createdAt: t - 14 * DAY,
      updatedAt: t - 2 * DAY,
      deletedAt: null,
    },
  ]
  runs = [
    {
      id: 'arun-demo0001',
      automationId: 'auto-demo0001',
      status: 'succeeded',
      trigger: 'schedule',
      scheduledFor: t - 3_600_000,
      startedAt: t - 3_600_000,
      endedAt: t - 3_598_000,
      sessionId: null,
      taskId: 'task-demo0001',
      stopReason: 'queued',
      error: null,
      summary: 'queued task task-demo0001: 每日依赖巡检',
      createdAt: t - 3_600_000,
    },
    {
      id: 'arun-demo0002',
      automationId: 'auto-demo0002',
      status: 'failed',
      trigger: 'schedule',
      scheduledFor: t - 2 * DAY,
      startedAt: t - 2 * DAY,
      endedAt: t - 2 * DAY + 12_000,
      sessionId: 'auto-arun-demo0002',
      taskId: null,
      stopReason: 'refusal',
      error: 'agent stopped: refusal',
      summary: null,
      createdAt: t - 2 * DAY,
    },
  ]
}

export function mockList(): Automation[] {
  seedOnce()
  return automations
    .filter((a) => a.deletedAt === null)
    .slice()
    .sort((a, b) => b.updatedAt - a.updatedAt)
}

export function mockGet(id: string): Automation {
  seedOnce()
  const row = automations.find((a) => a.id === id && a.deletedAt === null)
  if (!row) throw new Error(`not-found: automation ${id}`)
  return row
}

export function mockCreate(draft: AutomationDraft): Automation {
  seedOnce()
  validate(draft)
  const t = now()
  const row = draftToAutomation(draft, createId('auto'), t)
  automations.push(row)
  return row
}

export function mockUpdate(id: string, draft: AutomationDraft): Automation {
  seedOnce()
  validate(draft)
  const idx = automations.findIndex((a) => a.id === id && a.deletedAt === null)
  if (idx === -1) throw new Error(`not-found: automation ${id}`)
  const old = automations[idx]
  const t = now()
  const row = draftToAutomation(draft, old.id, old.createdAt)
  row.lastRunAt = old.lastRunAt
  row.lastRunStatus = old.lastRunStatus
  row.lastRunId = old.lastRunId
  row.unseenFailures = old.unseenFailures
  row.updatedAt = t
  automations[idx] = row
  return row
}

export function mockSetEnabled(id: string, enabled: boolean): Automation {
  seedOnce()
  const row = mockGet(id)
  row.enabled = enabled
  row.updatedAt = now()
  row.nextRunAt =
    enabled && row.triggerKind === 'schedule' ? nextRunSimple(row.cron, row.timezone) : null
  return row
}

export function mockSoftDelete(id: string): void {
  seedOnce()
  const row = automations.find((a) => a.id === id && a.deletedAt === null)
  if (!row) throw new Error(`not-found: automation ${id}`)
  row.deletedAt = now()
  row.nextRunAt = null
  row.updatedAt = now()
}

export function mockMarkSeen(): void {
  seedOnce()
  for (const a of automations) a.unseenFailures = 0
}

export function mockComputeNextRun(cron: string, timezone: string): NextRunPreview {
  const nextRunAt = nextRunSimple(cron, timezone)
  return {
    nextRunAt,
    timezoneSupported: timezone !== '' && tzOffset(timezone) !== 0 ? true : timezone === 'UTC',
    timezoneHasDst: ['Europe/London', 'Europe/Berlin', 'America/New_York', 'America/Los_Angeles'].includes(
      timezone,
    ),
  }
}

export function mockRuns(id: string, limit = 20): AutomationRun[] {
  seedOnce()
  return runs
    .filter((r) => r.automationId === id)
    .slice()
    .sort((a, b) => b.createdAt - a.createdAt)
    .slice(0, Math.max(1, limit))
}

/**
 * 演示用的「立即运行」：不真的起 agent，只造一条成功记录。
 *
 * - `enqueue_task` → 造一条 `queued task …` 摘要（不真的写任务板）
 * - `launch_session` → 造一条 `end_turn` 成功记录
 *
 * 真机行为由 Rust engine 负责。
 */
export function mockRunNow(id: string): string {
  seedOnce()
  const auto = mockGet(id)
  const t = now()
  const runId = createId('arun')
  const run: AutomationRun = {
    id: runId,
    automationId: auto.id,
    status: 'succeeded',
    trigger: 'manual',
    scheduledFor: null,
    startedAt: t,
    endedAt: t + 1_500,
    sessionId: auto.action === 'launch_session' ? `auto-${runId}` : null,
    taskId: auto.action === 'enqueue_task' ? createId('task') : null,
    stopReason: auto.action === 'enqueue_task' ? 'queued' : 'end_turn',
    error: null,
    summary:
      auto.action === 'enqueue_task'
        ? `queued task （演示）：${auto.name}`
        : '（演示）已跑完一轮',
    createdAt: t,
  }
  runs.push(run)
  auto.lastRunAt = t
  auto.lastRunStatus = 'succeeded'
  auto.lastRunId = runId
  auto.updatedAt = t
  return runId
}

export function mockCancelRun(runId: string): void {
  seedOnce()
  const run = runs.find((r) => r.id === runId)
  if (!run) throw new Error(`not-found: run ${runId}`)
  if (run.status !== 'running') return
  run.status = 'cancelled'
  run.endedAt = now()
  run.stopReason = 'cancelled'
}

/** 与 Rust `validate_draft` 同口径的最小校验 */
function validate(draft: AutomationDraft): void {
  if (!draft.name.trim()) throw new Error('io: 自动化名称不能为空')
  if (!draft.prompt.trim()) throw new Error('io: 提示词不能为空')
  if (!draft.projectId.trim()) throw new Error('io: 必须选择一个目标项目')
  if (!draft.rootPath.trim()) throw new Error('io: 目标项目缺少根目录')
  if (!draft.engineId.trim()) throw new Error('io: 必须选择引擎')
  if ((draft.triggerKind as TriggerKind) === 'schedule' && !draft.cron.trim()) {
    throw new Error('io: 定时触发必须填写 cron 表达式')
  }
}
