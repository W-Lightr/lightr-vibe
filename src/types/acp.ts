// ACP (Agent Client Protocol) 前端类型：按 research/acp-protocol.md v1 建模。
// update 变体用判别联合（sessionUpdate 字段）；反向 request 的 toolCall 是
// ToolCallUpdate 增量形状（协议 §6），全部字段容忍缺失。

import type { EngineWire } from './engine'

/** ACP 进程状态（acp://status 事件 / acp_status 命令） */
export type AcpProcessStatus = 'ready' | 'dead' | 'stopped'

export interface AcpStatusEvent {
  /** 连接 id = 本地会话 id（ses-xxxxxxxx）；每会话独立进程（design §8.1） */
  connId: string
  status: AcpProcessStatus
  reason?: string | null
}

/** session/update 载荷：agent_message_chunk / thought 共用形状 */
export interface AcpContentChunk {
  sessionUpdate: 'user_message_chunk' | 'agent_message_chunk' | 'agent_thought_chunk'
  messageId?: string
  content?: { type?: string; text?: string }
}

export type AcpToolKind =
  | 'read'
  | 'edit'
  | 'delete'
  | 'move'
  | 'search'
  | 'execute'
  | 'think'
  | 'fetch'
  | 'switch_mode'
  | 'other'

export type AcpToolCallStatus = 'pending' | 'in_progress' | 'completed' | 'failed'

/** tool_call / tool_call_update / request_permission.toolCall 的增量形状 */
export interface AcpToolCallUpdate {
  toolCallId?: string
  title?: string
  kind?: AcpToolKind
  status?: AcpToolCallStatus
  /** 整体替换语义（probe §3：每次带全量文本，前端覆盖不 append） */
  content?: unknown[]
  locations?: { path?: string }[]
  rawInput?: unknown
  rawOutput?: unknown
}

export interface AcpToolCallEvent extends AcpToolCallUpdate {
  sessionUpdate: 'tool_call' | 'tool_call_update'
}

export interface AcpUsageUpdateEvent {
  sessionUpdate: 'usage_update'
  used?: number
  size?: number
  cost?: { amount?: number; currency?: string }
}

/** 计划条目（ACP 标准 PlanEntry）：content 步骤描述，priority/status 可选 */
export type AcpPlanPriority = 'high' | 'medium' | 'low'
export type AcpPlanEntryStatus = 'pending' | 'in_progress' | 'completed'

export interface AcpPlanEntry {
  content: string
  priority?: AcpPlanPriority
  /** 步骤状态；缺失按待办处理 */
  status?: AcpPlanEntryStatus
}

/** plan：agent 主动推送的计划清单（流式增量按 entries 整体覆盖，不 append） */
export interface AcpPlanUpdateEvent {
  sessionUpdate: 'plan'
  entries?: AcpPlanEntry[]
}

/**
 * session_info_update：agent 原生会话标题 / 重试状态 / 压缩进度等。
 * `title` 为原生标题；重试/压缩信息放在 `_meta`（透传为 unknown，前端按需解析）。
 * `_meta` 可能在 update 内，也可能由 Rust 提到事件顶层（见 AcpUpdateEvent.meta）。
 */
export interface AcpSessionInfoUpdateEvent {
  sessionUpdate: 'session_info_update'
  title?: string
  _meta?: unknown
}

/** 会话可用命令（供输入 `/` 菜单 / 面板展示） */
export interface AcpAvailableCommand {
  name: string
  description?: string
}

export interface AcpAvailableCommandsEvent {
  sessionUpdate: 'available_commands_update'
  availableCommands?: AcpAvailableCommand[]
}

/** 当前模式（对齐 config_option_update 的 mode 项） */
export interface AcpCurrentModeUpdateEvent {
  sessionUpdate: 'current_mode_update'
  currentModeId?: string
}

/** config_option_update：agent 侧变更推送（切模型后 effort 选项可动态增减） */
export interface AcpConfigOptionUpdateEvent {
  sessionUpdate: 'config_option_update'
  configOptions?: AcpConfigOption[]
}

/** notice：ACP 1.9 Session Notices（agent 的网关警告等）。
 *  只有声明了 `session.notices` 能力才会以该种类下发；前端转 toast，不入 transcript。 */
export interface AcpNoticeUpdateEvent {
  sessionUpdate: 'notice'
  severity?: string
  title?: string
  description?: string
}

export type AcpUpdate =
  | AcpContentChunk
  | AcpToolCallEvent
  | AcpUsageUpdateEvent
  | AcpConfigOptionUpdateEvent
  | AcpPlanUpdateEvent
  | AcpSessionInfoUpdateEvent
  | AcpAvailableCommandsEvent
  | AcpCurrentModeUpdateEvent
  | AcpNoticeUpdateEvent

export interface AcpUpdateEvent {
  /** 连接 id = 本地会话 id（ses-xxxxxxxx）：事件按此路由到本地会话（design §8.1） */
  connId: string
  /** ACP 侧 sessionId（opencode ses_xxx）：消息归属用 */
  sessionId: string
  update: AcpUpdate
  /** Rust 透传的 `params._meta`（update 外，如 opencode retry 键）；缺失为 null。
   *  前端解析重试时两处都读（update._meta 优先，其次此 meta），见 sessionInfoMeta。 */
  meta?: unknown
}

/** acp://configRejected 事件载荷（Rust 在 acp_set_config 应答比对产生，非 wire 种类）：
 *  请求值被 agent 落成别的值 → 前端提示「该选项被 agent 调整」（AC6） */
export interface AcpConfigRejectedEvent {
  connId: string
  configId: string
  requested: string
  actual: string
}

/** session/new 响应的会话配置项（probe §2：model / mode select） */
export interface AcpConfigOptionValue {
  value: string
  name?: string
  description?: string
}

export interface AcpConfigOptionGroup {
  group?: string
  name?: string
  options?: AcpConfigOptionValue[]
}

export interface AcpConfigOption {
  id: string
  name?: string
  description?: string
  category?: string
  type?: 'select' | 'boolean'
  currentValue?: unknown
  options?: (AcpConfigOptionValue | AcpConfigOptionGroup)[]
}

/** session/request_permission 选项 */
export interface AcpPermissionOption {
  optionId: string
  name?: string
  /** 选项语义：allow_once/allow_always/reject_once/reject_always（未知 agent 可能给别的）
   *  —— 回应时按此优先级挑选项，不按数组顺序 */
  kind?: 'allow_once' | 'allow_always' | 'reject_once' | 'reject_always' | string
  /** 选项级 `_meta`（含 `permission.changes[]`：选中该选项会授予什么/持续多久） */
  meta?: unknown
}

export interface AcpPermissionEvent {
  /** 连接 id = 本地会话 id：答复时精确定位该会话进程（design §8.1） */
  connId: string
  requestId: string
  sessionId: string
  toolCall: AcpToolCallUpdate
  options: AcpPermissionOption[]
  /** 请求级 `_meta`（`permission.{title,description,defaultToNo}` / `claudeCode.title`）；
   *  缺失 → 事件里为 null */
  meta?: unknown
}

/** elicitation/create 题型（Questions 型）：单选 / 多选 / 自由文本（Rust acp_question.rs 同形） */
export type AcpQuestionKind = 'single' | 'multi' | 'text'

/** elicitation 候选项：value 回传 content，label 展示 */
export interface AcpQuestionOption {
  value: string
  label: string
}

/** 一道 elicitation 提问（本期一次一问） */
export interface AcpQuestion {
  /** = form properties 的键：回传答案的 content key */
  id: string
  prompt: string
  kind: AcpQuestionKind
  /** single/multi 有；text 为空数组 */
  options: AcpQuestionOption[]
}

/** acp://question 事件载荷（Rust emit，connId 路由） */
export interface AcpQuestionEvent {
  /** 连接 id = 本地会话 id（ses-xxxxxxxx）：按此路由到本地会话（design §8.1） */
  connId: string
  requestId: string
  /** ACP 侧 sessionId（agent-side ses_xxx），仅归属用 */
  sessionId: string
  questions: AcpQuestion[]
}

/** Rust 命令返回（camelCase，tauri-bridge spec §2） */
export interface AcpStartResult {
  status: AcpProcessStatus
  protocolVersion: number
  agentInfo?: { name: string; version: string } | null
}

export interface AcpSessionResult {
  sessionId: string
  configOptions: AcpConfigOption[] | null
}

export type AcpStopReason =
  | 'end_turn'
  | 'max_tokens'
  | 'max_turn_requests'
  | 'refusal'
  | 'cancelled'

export interface AcpPromptResult {
  stopReason: AcpStopReason | string
  usage: {
    inputTokens?: number
    outputTokens?: number
    totalTokens?: number
    /** opencode 扩展：缓存读取 token（可能缺省，容忍缺失） */
    cachedReadTokens?: number
    /** opencode 扩展：缓存写入 token（可能缺省，容忍缺失） */
    cachedWriteTokens?: number
  } | null
}

/** 会话索引条目（~/.lightr-vibe/lightr-vibe/sessions-index.json，PRD R3） */
export interface SessionIndexEntry {
  id: string
  title: string
  kind: 'folder' | 'tmp'
  projectId?: string
  cwd?: string
  acpSessionId?: string
  model: string
  mode?: string
  /** 会话绑定的引擎 wire 标识；旧索引缺该字段 → 加载时回退 open_code（AC5） */
  engineId?: EngineWire
  createdAt: number
  updatedAt: number
}
