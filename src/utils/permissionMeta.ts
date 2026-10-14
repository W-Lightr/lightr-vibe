// ACP 权限请求的 `_meta` 解析（纯函数，无副作用，便于单测）。
// 参考 codeg `src/lib/permission-request.ts`（getCodegSource）。原则：
// - **只在能完整理解时才展示**（`version === 1` 门槛）；形状不认识 → 返回空，不半懂展示。
// - 一切从最宽松的 `unknown` 解码，缺失/类型不符一律安全回退。
//
// 数据来源两处：
//   1. 事件级 `event.meta`（Rust 把请求级 `params._meta` 原样透传）——codex/claude 的
//      `permission.{title,description,defaultToNo}` 与 `claudeCode.title` 多在此。
//   2. `event.toolCall._meta`（adapter 自己塞进 toolCall 的）——兼容 codeg 的读取位。
// 两个来源都查，事件级优先。

import type { AcpPermissionEvent, AcpPermissionOption } from '../types/acp'

/** 选项授予内容的持续范围（`_meta.permission.changes[].lifetime` 归一化） */
export type PermissionChangeScope =
  | 'session'
  | 'process'
  | 'user'
  | 'project'
  | 'project_local'
  | 'persistent'

export interface PermissionOptionChange {
  description: string
  scope: PermissionChangeScope | null
}

/** 仿 codeg：渲染上限，防止病态列表刷屏 */
const MAX_CHANGES = 6
const MAX_CHANGE_CHARS = 200

// ── 宽松解码原语 ─────────────────────────────────────────────
function asObject(v: unknown): Record<string, unknown> | null {
  return v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : null
}
function pickString(obj: Record<string, unknown> | null, keys: string[]): string | null {
  if (!obj) return null
  for (const k of keys) {
    const v = obj[k]
    if (typeof v === 'string' && v.trim()) return v
  }
  return null
}

/** 请求级 `_meta` 合并视图：优先事件级 `event.meta`，回退 `toolCall._meta` */
function permissionMeta(event: AcpPermissionEvent): {
  meta: Record<string, unknown> | null
  permission: Record<string, unknown> | null
} {
  const eventMeta = asObject(event.meta)
  const toolMeta = asObject((event.toolCall as { _meta?: unknown })._meta)
  const meta = eventMeta ?? toolMeta
  return { meta, permission: asObject(meta?.['permission']) }
}

/** `permission.changes[].lifetime` → scope（仅 `version===1` 语义下的 lifetime object） */
function parseChangeScope(lifetime: unknown): PermissionChangeScope | null {
  const record = asObject(lifetime)
  const scope = pickString(record, ['scope'])
  if (scope === 'session') return 'session'
  if (scope === 'process') return 'process'
  if (scope !== 'persistent') return null
  switch (pickString(record, ['storage'])) {
    case 'user':
      return 'user'
    case 'project':
      return 'project'
    case 'project_local':
      return 'project_local'
    default:
      return 'persistent'
  }
}

/** `permission` 块的扁平 `description`（仅 `version===1`） */
function permissionBlockDescription(permission: unknown): string | null {
  const record = asObject(permission)
  if (!record || record.version !== 1) return null
  const description = pickString(record, ['description'])
  return description ? description.slice(0, MAX_CHANGE_CHARS) : null
}

/** `permission.<key>` 严格布尔（仅字面 `true`；仅 `version===1`） */
function parsePermissionFlag(permission: unknown, key: string): boolean {
  const record = asObject(permission)
  if (!record || record.version !== 1) return false
  return record[key] === true
}

// ── 对外投影 ─────────────────────────────────────────────────

/** 权限卡标题：`_meta.claudeCode.title` → `_meta.permission.title` → `toolCall.title` →
 *  `toolCall.toolCallId`。当标题恰等于要执行的命令时，回退到工具标题（避免把命令显示两遍）。 */
export function parsePermissionTitle(event: AcpPermissionEvent): string {
  const { meta, permission } = permissionMeta(event)
  const command = parsePermissionCommand(event)
  const heading =
    pickString(asObject(meta?.['claudeCode']), ['title']) ??
    pickString(permission, ['title']) ??
    null
  if (heading && command && heading === command) {
    return event.toolCall.title ?? heading
  }
  return heading ?? event.toolCall.title ?? event.toolCall.toolCallId ?? '未知工具'
}

/** agent 自述的请求理由（`_meta.permission.description`，仅 `version===1`） */
export function parsePermissionDescription(event: AcpPermissionEvent): string | null {
  const { permission } = permissionMeta(event)
  return permissionBlockDescription(permission)
}

/** 「不该被误触批准」标记（`_meta.permission.defaultToNo === true`） */
export function isDefaultToNo(event: AcpPermissionEvent): boolean {
  const { permission } = permissionMeta(event)
  return parsePermissionFlag(permission, 'defaultToNo')
}

/** 要执行的命令（`rawInput.command`，若为字符串） */
export function parsePermissionCommand(event: AcpPermissionEvent): string | null {
  const raw = asObject(event.toolCall.rawInput)
  const cmd = raw?.['command']
  return typeof cmd === 'string' && cmd.trim() ? cmd : null
}

/**
 * 选项级「选中会授予什么」：`option.meta.permission` 两形态
 *  - `{version:1, changes:[{description, lifetime?}]}`（claude-agent-acp 0.64.1–0.72.0 /
 *    codex-acp 1.1.8–1.6.2）
 *  - `{version:1, description}`（codex-acp ≥1.7.0，单条无 scope）
 * 无 `_meta` / `version!==1` → `[]`（不半懂展示）。
 */
export function parseOptionChanges(option: AcpPermissionOption): PermissionOptionChange[] {
  const meta = asObject(option.meta)
  const permission = asObject(meta?.['permission'])
  if (!permission || permission.version !== 1) return []

  const changes = permission['changes']
  if (!Array.isArray(changes)) {
    const description = permissionBlockDescription(permission)
    return description ? [{ description, scope: null }] : []
  }

  const out: PermissionOptionChange[] = []
  for (const change of changes) {
    if (out.length >= MAX_CHANGES) break
    const record = asObject(change)
    const description = pickString(record, ['description'])
    if (!description) continue
    out.push({
      description: description.slice(0, MAX_CHANGE_CHARS),
      scope: parseChangeScope(record?.['lifetime']),
    })
  }
  return out
}

/** 选项 `kind` → 中文按钮文案（`option.name` 缺失时的回退，PRD R4 / design §4.3） */
export const KIND_LABEL: Record<string, string> = {
  allow_once: '允许一次',
  allow_always: '本会话总是允许',
  reject_once: '拒绝',
  reject_always: '始终拒绝',
}

/** 选项按钮文案：**优先中文**（按 kind 映射，UI 统一中文）；未知 kind 才退回
 *  agent 给的 `name`，再退回 `optionId`。opencode 默认发英文名（Allow once/…），
 *  直接用它会让界面中英混排，故已知 kind 一律走我们的中文文案。 */
export function optionLabel(option: AcpPermissionOption): string {
  if (option.kind && KIND_LABEL[option.kind]) return KIND_LABEL[option.kind]
  if (option.name && option.name.trim()) return option.name
  return option.optionId
}

/** scope → 中文徽章文案（无 scope → 不显示徽章） */
export const CHANGE_SCOPE_LABEL: Record<PermissionChangeScope, string> = {
  session: '本会话',
  process: '本进程',
  user: '永久（用户）',
  project: '永久（项目）',
  project_local: '永久（本项目）',
  persistent: '永久',
}

/**
 * 从 `rawInput` 取 diff 文本（用于 edit 类审批的 diff 预览）。
 * 覆盖形态：
 *  - string：本身即 diff（含 `@@` 或 +/- 行）→ 原样返回
 *  - object：`patch` / `diff` / `file_text` / `new_string` 等字段
 * 找不到可渲染的 diff → null（调用方回退 JSON 摘要）。
 */
export function extractDiff(rawInput: unknown): string | null {
  if (typeof rawInput === 'string') {
    return looksLikeDiff(rawInput) ? rawInput : null
  }
  const obj = asObject(rawInput)
  if (!obj) return null
  for (const key of ['patch', 'diff', 'unified_diff', 'unifiedDiff', 'file_text', 'new_string']) {
    const v = obj[key]
    if (typeof v === 'string' && looksLikeDiff(v)) return v
  }
  return null
}

/** 是否像一段 unified diff / 补丁（有 `@@` 或行首 +/- 标记） */
function looksLikeDiff(text: string): boolean {
  if (text.includes('@@')) return true
  return /(^|\n)[+-][^\n]*/.test(text)
}
