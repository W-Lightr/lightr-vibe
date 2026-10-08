// 浏览器 dev 环境的 OpenCode mock 数据（design §2.1 / PRD R3）。
// why: 前端在非 Tauri 环境无法真正 spawn 进程或读写用户目录；
// 这里提供固定、全 PASS 的假数据，界面顶部会明示「浏览器模式：数据为 mock」。

import type { ConfigReadResult, ConfigWriteResult, DetectResult, EnvFileResult } from '../types/agent'

/** mock 版本号与本机实测一致（research §1） */
export const MOCK_VERSION = '1.18.35'

/** mock opencode.jsonc 文本：含注释与 provider/model/permission 结构，覆盖真实形态 */
export const MOCK_CONFIG_TEXT = `{
  // Lightr-vibe 浏览器模式示例配置（mock，不会写入磁盘）
  "$schema": "https://opencode.ai/config.json",
  "provider": {
    "GS": {
      "npm": "@ai-sdk/anthropic",
      "name": "GS",
      "options": {
        "baseURL": "https://ov.lightr.cn/llm/v1",
        "apiKey": "sk-mock1234567890abcdef"
      },
      "models": {
        "Qwen3-Coder-Next-FP8": {
          "name": "Qwen3-Coder-Next-FP8",
          "limit": { "context": 230000, "output": 230000 },
          "reasoning": true,
          "tool_call": true
        }
      },
      "whitelist": ["Qwen3-Coder-Next-FP8"]
    },
    "openai": {
      "npm": "@ai-sdk/openai",
      "name": "OpenAI",
      "options": {
        "baseURL": "https://api.openai.com/v1",
        "apiKey": "sk-mockOpenAIKey0001"
      },
      "models": {
        "gpt-5-mini": {
          "name": "gpt-5-mini",
          "limit": { "context": 400000, "output": 128000 },
          "reasoning": true,
          "tool_call": true
        }
      }
    }
  },
  "model": "GS/Qwen3-Coder-Next-FP8",
  "small_model": "openai/gpt-5-mini",
  "permission": {
    "edit": "ask",
    "bash": "allow"
  }
}
`

/** mock 检测：全部通过 */
export function mockDetect(): DetectResult {
  return {
    installed: true,
    version: MOCK_VERSION,
    binPath: 'C:\\Users\\mock\\.version-fox\\sdks\\nodejs\\opencode.cmd',
    platform: 'win32 / x86_64 (mock)',
    acpOk: true,
    configPath: '~/.config/opencode/opencode.jsonc (mock)',
    configExists: true,
    detail: null,
  }
}

/** mock 内存态：配置与环境变量在会话内可读写（刷新页面即重置） */
let mockConfigText = MOCK_CONFIG_TEXT
let mockConfigMtime = Date.now()
let mockEnvText: string | null = null

export function mockReadConfig(): ConfigReadResult {
  return { text: mockConfigText, mtimeMs: mockConfigMtime, path: '~/.config/opencode/opencode.jsonc (mock)' }
}

export function mockWriteConfig(text: string, expectedMtime: number | null): ConfigWriteResult {
  // mock 也模拟 mtime 冲突保护语义
  if (expectedMtime !== null && expectedMtime !== mockConfigMtime) {
    throw new Error('conflict')
  }
  mockConfigText = text
  mockConfigMtime = Date.now()
  return { mtimeMs: mockConfigMtime }
}

export function mockReadEnv(): EnvFileResult {
  return { text: mockEnvText, path: '~/.lightr-vibe/agent-env.json (mock)' }
}

export function mockWriteEnv(text: string): void {
  mockEnvText = text
}
