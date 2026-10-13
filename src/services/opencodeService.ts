// OpenCode 服务层：检测 + 配置读写的唯一入口（design §1 边界原则）。
// isTauri() 双通道：Tauri 环境走 Rust invoke（真实进程/文件），
// 浏览器 dev 环境整体回退 opencodeMock（内存态，界面明示 mock）。
// 组件与 store 不直接感知通道差异；写锁在 store（PRD R7 并发保护）。

import { invoke } from '@tauri-apps/api/core'
import type {
  ConfigReadResult,
  ConfigWriteResult,
  DetectResult,
  EnvFileResult,
  ModelView,
  OpencodeConfigView,
  PermissionAction,
  ProviderView,
} from '../types/agent'
import { parseJsonc } from '../utils/jsonc'
import { isTauri } from '../utils/isTauri'
import {
  mockDetect,
  mockReadConfig,
  mockReadEnv,
  mockWriteConfig,
  mockWriteEnv,
} from './opencodeMock'

export const opencodeService = {
  /** 检测引擎安装/版本/ACP 能力（engineId 缺省回退 opencode） */
  async detect(engineId?: string): Promise<DetectResult> {
    if (!isTauri()) return mockDetect()
    return invoke<DetectResult>('agent_detect', { engineId })
  },

  /** 读配置文件原文 + mtime（不解析，解析在 normalizeConfig） */
  async readConfig(engineId?: string): Promise<ConfigReadResult> {
    if (!isTauri()) return mockReadConfig()
    return invoke<ConfigReadResult>('agent_read_config', { engineId })
  },

  /** 写回补丁后的完整文本；mtime 不一致抛「conflict」错误（Rust 侧校验） */
  async writeConfig(
    text: string,
    expectedMtime: number | null,
    engineId?: string,
  ): Promise<ConfigWriteResult> {
    if (!isTauri()) return mockWriteConfig(text, expectedMtime)
    return invoke<ConfigWriteResult>('agent_write_config', {
      engineId,
      text,
      expectedMtime,
    })
  },

  /** 读应用级环境变量文件 ~/.lightr-vibe/agent-env.json */
  async readEnvFile(): Promise<EnvFileResult> {
    if (!isTauri()) return mockReadEnv()
    return invoke<EnvFileResult>('agent_read_env_file')
  },

  /** 写应用级环境变量文件（纯 JSON） */
  async writeEnvFile(text: string): Promise<void> {
    if (!isTauri()) {
      mockWriteEnv(text)
      return
    }
    await invoke('agent_write_env_file', { text })
  },
}

/** apiKey 掩码：保留前 5 / 后 2 字符（PRD R4，如 sk-1i…8A） */
export function maskApiKey(key: string): string {
  if (key.length <= 7) return '••••'
  return `${key.slice(0, 5)}…${key.slice(-2)}`
}

/** 从 JSONC 原文提取 provider/模型/权限的归一化视图（design §2.2） */
export function normalizeConfig(text: string): OpencodeConfigView {
  const raw = parseJsonc(text)
  const root = isRecord(raw) ? raw : {}

  const providers: ProviderView[] = []
  const providerMap = isRecord(root.provider) ? root.provider : {}
  for (const [name, value] of Object.entries(providerMap)) {
    if (!isRecord(value)) continue
    const options = isRecord(value.options) ? value.options : {}
    const models: ModelView[] = []
    const modelMap = isRecord(value.models) ? value.models : {}
    for (const [modelId, modelValue] of Object.entries(modelMap)) {
      const model = isRecord(modelValue) ? modelValue : {}
      const limit = isRecord(model.limit) ? model.limit : {}
      models.push({
        id: modelId,
        label: `${name}/${modelId}`,
        context: typeof limit.context === 'number' ? limit.context : undefined,
        reasoning: typeof model.reasoning === 'boolean' ? model.reasoning : undefined,
        // 官方 schema 键为 snake_case（tool_call）
        toolCall: typeof model.tool_call === 'boolean' ? model.tool_call : undefined,
      })
    }
    const apiKeyPlain = typeof options.apiKey === 'string' ? options.apiKey : null
    providers.push({
      // key = 配置对象键（补丁路径用），name = 显示名（可能与键不同）
      key: name,
      name: typeof value.name === 'string' && value.name ? value.name : name,
      npm: typeof value.npm === 'string' ? value.npm : '',
      baseURL: typeof options.baseURL === 'string' ? options.baseURL : '',
      apiKeyMasked: apiKeyPlain ? maskApiKey(apiKeyPlain) : null,
      apiKeyPlain,
      models,
      whitelist: Array.isArray(value.whitelist)
        ? value.whitelist.filter((w): w is string => typeof w === 'string')
        : [],
    })
  }

  // permission：本期 UI 只识别简单三态值；对象映射（细粒度规则）忽略不展示，
  // 且写回时不覆盖（patchConfig 只改用户选择过的键）
  const permissions: Record<string, PermissionAction> = {}
  const permissionRaw = root.permission
  if (isPermissionAction(permissionRaw)) {
    // 官方 schema 允许 permission 为单一 action 字符串（作用于所有键），
    // 归一化为通配键供 UI 展示，写回时按具体键展开
    permissions['*'] = permissionRaw
  } else if (isRecord(permissionRaw)) {
    for (const [key, value] of Object.entries(permissionRaw)) {
      if (isPermissionAction(value)) permissions[key] = value
    }
  }

  return {
    model: typeof root.model === 'string' ? root.model : null,
    smallModel: typeof root.small_model === 'string' ? root.small_model : null,
    providers,
    permissions,
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function isPermissionAction(value: unknown): value is PermissionAction {
  return value === 'ask' || value === 'allow' || value === 'deny'
}
