// Agent 管理域模型：Agent 定义、检测/预检结果、OpenCode 配置视图。
// 全部使用字符串字面量联合（type-safety.md：禁用 TS enum）。

/** 权限动作：官方 schema 的 PermissionRuleConfig 简单三态 */
export type PermissionAction = 'ask' | 'allow' | 'deny'

/**
 * 官方 schema 的 permission 键全集（research §3，2026-10-08 快照）。
 * key 为写入 opencode.jsonc 的原始键名；label 为界面中文名。
 */
export const PERMISSION_KEYS: { key: string; label: string }[] = [
  { key: 'read', label: '读取文件' },
  { key: 'edit', label: '编辑文件' },
  { key: 'glob', label: '文件匹配' },
  { key: 'grep', label: '内容搜索' },
  { key: 'list', label: '列目录' },
  { key: 'bash', label: '执行命令' },
  { key: 'task', label: '子任务' },
  { key: 'external_directory', label: '外部目录' },
  { key: 'todowrite', label: '待办写入' },
  { key: 'question', label: '向用户提问' },
  { key: 'webfetch', label: '网页抓取' },
  { key: 'websearch', label: '网络搜索' },
  { key: 'lsp', label: 'LSP 语言服务' },
  { key: 'doom_loop', label: '死循环保护' },
  { key: 'skill', label: '技能' },
]

/** 预检项状态 */
export type PrecheckStatus = 'pass' | 'fail' | 'running' | 'unknown'

/** 单条预检项（版本状态 / Platform / 配置文件 / ACP 能力） */
export interface PrecheckItem {
  id: string
  label: string
  status: PrecheckStatus
  /** 明细文本（版本号、路径、失败原因等） */
  detail: string
}

/** Rust `agent_detect(engineId)` 返回值（design §2.1 契约；engineId 缺省回退 opencode） */
export interface DetectResult {
  installed: boolean
  version: string | null
  binPath: string | null
  platform: string
  acpOk: boolean
  configPath: string
  configExists: boolean
  /** 检测过程问题明细（超时/退出码异常），无问题为 null */
  detail: string | null
}

/** Rust `agent_read_config(engineId)` 返回值 */
export interface ConfigReadResult {
  text: string | null
  mtimeMs: number | null
  path: string
}

/** Rust `agent_write_config(engineId, text, expectedMtime)` 返回值 */
export interface ConfigWriteResult {
  mtimeMs: number
}

/** Rust `agent_read_env_file` 返回值 */
export interface EnvFileResult {
  text: string | null
  path: string
}

/** Provider 下单个模型的视图 */
export interface ModelView {
  /** 模型 id（provider.models 的键） */
  id: string
  /** `provider/model-id`，即顶层 model 字段的写入值 */
  label: string
  context?: number
  reasoning?: boolean
  toolCall?: boolean
}

/** opencode.jsonc 中单个 provider 的视图（apiKey 明文仅存内存，UI 默认掩码） */
export interface ProviderView {
  /** 配置文件中 provider 对象的键名；写回补丁路径必须用它（可能与显示名 name 不同） */
  key: string
  name: string
  /** npm SDK 包名（如 @ai-sdk/anthropic） */
  npm: string
  baseURL: string
  /** 掩码展示值，如 sk-1i…8A；无 apiKey 时为 null */
  apiKeyMasked: string | null
  apiKeyPlain: string | null
  models: ModelView[]
  /** 白名单模型 id 列表（本期仅展示，不做启用开关） */
  whitelist: string[]
}

/** opencode.jsonc 归一化视图（design §2.2） */
export interface OpencodeConfigView {
  /** 顶层 model（provider/model-id） */
  model: string | null
  /** 顶层 small_model（snake_case，官方 schema 键名） */
  smallModel: string | null
  providers: ProviderView[]
  /** permission 对象中已显式设置的简单三态键值 */
  permissions: Record<string, PermissionAction>
}

/** Agent 列表条目定义（本期仅 OpenCode 可配置，其余为禁用占位行） */
export interface AgentDef {
  id: string
  name: string
  /** lobehub 图标（?raw 内联 SVG 文本） */
  iconSvg: string
  /** 是否本期可配置/可检测；false = 禁用占位行「即将推出」 */
  enabled: boolean
  description: string
}

/** Agent 在列表中的检测状态汇总（由 DetectResult 派生） */
export type AgentStatus = 'pass' | 'fail' | 'detecting' | 'not-installed' | 'unknown'

/** 环境变量条目（行编辑器的 KEY=VALUE 对） */
export interface EnvVar {
  key: string
  value: string
}
