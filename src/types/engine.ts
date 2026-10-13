// 引擎（Agent 类型）域模型：wire 标识与注册表下发形状。
// 照 Rust 侧 AgentType::as_wire（wire 名是持久化契约，不可变）。

/** 引擎 wire 标识：写入 Session.engineId / SessionIndexEntry.engineId。 */
export type EngineWire = 'open_code'

/** 已知引擎 wire 列表（前端静态回退用；权威列表来自 acp_list_agents）。 */
export const BUILTIN_ENGINE_WIRES: EngineWire[] = ['open_code']

/** 默认引擎 wire（设置页不可用时的兜底）。 */
export const DEFAULT_ENGINE_WIRE: EngineWire = 'open_code'

/** Rust `acp_list_agents` 返回项（camelCase 契约）。 */
export interface AcpAgentInfo {
  /** 引擎 wire 标识（Rust AgentType 序列化） */
  agentType: EngineWire
  name: string
  description: string
  /** 是否启用（预留；本期恒 true） */
  enabled: boolean
  /** 排序权重（预留；前端按 sortOrder + name 排序） */
  sortOrder: number
  /** 是否检测通过（PATH 解析到可执行文件） */
  available: boolean
  installedVersion: string | null
}
