// 会话与消息的核心数据模型

export type MessageRole = 'user' | 'assistant'
export type MessageType = 'text' | 'thinking' | 'error'
export type MessageStatus = 'pending' | 'streaming' | 'done' | 'stopped' | 'error'
export type SessionStatus = 'idle' | 'generating' | 'error'
export type ExecutionMode = 'auto' | 'confirm' | 'plan'

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
}

export type SessionKind = 'folder' | 'tmp'

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
  /** 会话类型：文件夹会话 / 临时聊天会话 */
  kind: SessionKind
  /** 所属项目（工作区）；临时会话无项目 */
  projectId?: string
  /** 临时会话的落盘路径（用户数据目录 /.lightr-vibe/tmpsession） */
  storagePath?: string
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
