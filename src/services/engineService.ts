// 引擎服务层：acp_list_agents + 默认引擎读写的唯一入口（tauri-bridge spec §2 边界）。
// isTauri() 双通道：Tauri 走 Rust invoke，浏览器 dev 走内存 mock。
// 组件/store 不直接 invoke；默认引擎持久化到 ~/.lightr-vibe/lightr-vibe/settings.json。

import { invoke } from '@tauri-apps/api/core'
import type { AcpAgentInfo, EngineWire } from '../types/engine'
import { BUILTIN_ENGINE_WIRES, DEFAULT_ENGINE_WIRE } from '../types/engine'
import { isTauri } from '../utils/isTauri'

/**
 * 浏览器 mock：静态引擎列表（3 个内置引擎）。
 * `available` 与 mockDetect 口径一致：claude/codex 的外部适配器需用户自装
 * （mockDetect 返回 installed:false），故这里 available=false + 无版本号。
 */
function mockList(): AcpAgentInfo[] {
  return [
    {
      agentType: 'open_code',
      name: 'OpenCode',
      description: '支持 ACP（Agent Client Protocol）的开源编码 Agent',
      enabled: true,
      sortOrder: 0,
      available: true,
      installedVersion: '1.18.35 (mock)',
    },
    {
      agentType: 'claude_code',
      name: 'Claude Code',
      description: 'Anthropic Claude 的 ACP 适配器（用户自装 claude-agent-acp）',
      enabled: true,
      sortOrder: 1,
      available: false,
      installedVersion: null,
    },
    {
      agentType: 'codex',
      name: 'Codex CLI',
      description: 'OpenAI Codex 的 ACP 适配器（用户自装 codex-acp）',
      enabled: true,
      sortOrder: 2,
      available: false,
      installedVersion: null,
    },
  ]
}

/** mock 默认引擎内存态（刷新页面重置） */
let mockDefaultEngineId: EngineWire = DEFAULT_ENGINE_WIRE

export const engineService = {
  /** 列出所有内置引擎（含可用性） */
  async list(): Promise<AcpAgentInfo[]> {
    if (!isTauri()) return mockList()
    return invoke<AcpAgentInfo[]>('acp_list_agents')
  },

  /** 读取默认引擎 wire 值 */
  async defaultGet(): Promise<EngineWire> {
    if (!isTauri()) return mockDefaultEngineId
    const v = await invoke<string>('engine_default_get')
    return (BUILTIN_ENGINE_WIRES as string[]).includes(v)
      ? (v as EngineWire)
      : DEFAULT_ENGINE_WIRE
  },

  /** 写默认引擎 wire 值 */
  async defaultSet(engineId: EngineWire): Promise<void> {
    if (!isTauri()) {
      mockDefaultEngineId = engineId
      return
    }
    await invoke('engine_default_set', { engineId })
  },
}
