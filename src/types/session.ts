// 会话与消息的核心数据模型

import type { AcpAvailableCommand, AcpConfigOption, AcpPlanEntry } from './acp'
import type { EngineWire } from './engine'

export type MessageRole = 'user' | 'assistant'
export type MessageType = 'text' | 'thinking' | 'error'
export type MessageStatus = 'pending' | 'streaming' | 'done' | 'stopped' | 'error'
export type SessionStatus = 'idle' | 'generating' | 'error'
export type ExecutionMode = 'auto' | 'confirm' | 'plan'

/** 错误消息卡类别：决定文案与「去设置」按钮（PRD R4） */
export type MessageErrorKind = 'dead' | 'auth' | 'timeout' | 'generic'

export interface ToolCall {
  id: string
  name: string
  input: string
  result?: string
  status: 'running' | 'done' | 'error'
}

export interface TokenUsage {
  input: number
  output: number
  total: number
  /** 上下文窗口容量（token） */
  contextWindow: number
  /** 缓存读取 token（opencode usage.cachedReadTokens，缺省 0） */
  cachedRead: number
  /** 缓存写入 token（opencode usage.cachedWriteTokens，缺省 0） */
  cachedWrite: number
}

export interface Message {
  id: string
  sessionId: string
  role: MessageRole
  type: MessageType
  content: string
  status: MessageStatus
  createdAt: number
  /** 同一次问答轮次的关联 ID（用户消息 / 执行过程 / 助手回复共享） */
  turnId?: string
  toolCalls?: ToolCall[]
  /** 错误消息类别（type === 'error' 时有意义）：决定文案与「去设置」按钮 */
  errorKind?: MessageErrorKind
}

export type SessionKind = 'folder' | 'tmp'

/** 可重试错误状态（session_info_update 的 `_meta`）：渲染重试横幅，非失败卡（AC5）。
 *  均 in-memory，不落盘（重试是瞬态过程）。 */
export interface SessionRetryState {
  /** 第几次重试（agent 上报；缺省 undefined） */
  attempt?: number
  /** 错误摘要（可能缺省） */
  message?: string
  /** 下次重试时间戳（ms；opencode nextRetryAt，缺省 undefined） */
  nextRetryAt?: number
}

/** 新建会话的目标：绑定已有文件夹 / 临时会话（延迟入列，PRD R2/D5）。
 * 来源 = 新建弹窗的文件夹下拉；文件夹内新建不走弹窗，直接构造 folder target。 */
export type NewSessionTarget = { kind: 'folder'; projectId: string } | { kind: 'tmp' }

export interface Session {
  id: string
  title: string
  model: string
  status: SessionStatus
  createdAt: number
  updatedAt: number
  gitBranch: string
  extensionId: string
  tokenUsage: TokenUsage
  messages: Message[]
  /** 会话绑定的引擎 wire 标识（创建时确定，会话内不切换）；旧数据缺省 open_code */
  engineId: EngineWire
  /** 会话类型：文件夹会话 / 临时聊天会话 */
  kind: SessionKind
  /** 所属项目（工作区）；临时会话无项目 */
  projectId?: string
  /** 临时会话的落盘路径（~/.lightr-vibe/lightr-vibe/tmpsession） */
  storagePath?: string
  /** 对应的 ACP 会话 ID（session/new 返回）；未建连时为 undefined */
  acpSessionId?: string
  /** ACP 会话工作目录（项目 rootPath 或 tmpsession 目录） */
  cwd?: string
  /** session/new 返回的会话级配置项（model/mode select）；MessageComposer 消费 */
  configOptions?: AcpConfigOption[]
  /** agent 主动推送的计划清单（plan update，整体覆盖）；PlanCard 消费 */
  planEntries?: AcpPlanEntry[]
  /** 可重试错误状态（重试横幅数据源）；非重试中为 undefined */
  retryState?: SessionRetryState
  /** agent 可用命令列表（available_commands_update）；供 `/` 菜单/面板（AC7） */
  availableCommands?: AcpAvailableCommand[]
  /** 当前模式 id（current_mode_update）；对齐 mode config option（AC7） */
  currentModeId?: string
  /** ACP 会话 mode（D3：仅记录不暴露 UI，恒 build） */
  mode?: string
  /** 消息是否已从磁盘懒加载（索引恢复后打开标签才读，design §4） */
  messagesLoaded?: boolean
}

/** 中央工作区打开的标签：会话标签或文件编辑标签 */
export interface OpenTab {
  id: string
  kind: 'session' | 'file'
  sessionId?: string
  filePath?: string
  fileName?: string
  projectId?: string
}

/** 工作区 UI 状态（布局 / 标签 / 选择项） */
export interface WorkspaceState {
  activeSessionId: string | null
  openSessionIds: string[]
  sidebarVisible: boolean
  inspectorVisible: boolean
  selectedModel: string
  executionMode: ExecutionMode
  generatingSessionIds: string[]
}
