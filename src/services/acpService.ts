// ACP 服务层：acp_* 命令 invoke + acp://* 事件 listen 的唯一入口
// （tauri-bridge spec §2：组件/store 不直接 invoke）。
// isTauri() 双通道：Tauri 走 Rust 真实进程；浏览器 dev 走 acpMock
// （现有 conversationService MockEvent 流翻译为 ACP update 形状，design D5）。
// 事件三订阅为模块单例：init() 幂等，handler 注册在 acpEvents（update/permission/status）。

import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  AcpConfigOption,
  AcpPermissionEvent,
  AcpPromptResult,
  AcpSessionResult,
  AcpStartResult,
  AcpStatusEvent,
  AcpUpdateEvent,
} from '../types/acp'
import { isTauri } from '../utils/isTauri'
import { dispatchPermission, dispatchStatus, dispatchUpdate } from './acpEvents'
import { acpMock } from './acpMock'

let unlistenAll: UnlistenFn[] | null = null
let initPromise: Promise<void> | null = null

/** set_config_option 响应归一化：规范 §8 返回「全量最新 configOptions」——
 * Rust 侧原样透传 JSON（实测可能是裸数组或 {configOptions} 对象两种形态），
 * mock 通道返回 {configOptions}；统一抽取数组，双通道消费面一致 */
function extractConfigOptions(raw: unknown): AcpConfigOption[] | null {
  if (Array.isArray(raw)) return raw as AcpConfigOption[]
  if (raw && typeof raw === 'object' && 'configOptions' in raw) {
    const opts = (raw as { configOptions?: unknown }).configOptions
    if (Array.isArray(opts)) return opts as AcpConfigOption[]
  }
  return null
}

/** 初始化三事件订阅（模块单例，App 启动后调用；浏览器通道无 Tauri 事件，mock 直接 dispatch） */
export function initAcpListeners(): Promise<void> {
  if (!isTauri()) return Promise.resolve()
  if (!initPromise) {
    initPromise = (async () => {
      const [u1, u2, u3] = await Promise.all([
        listen<AcpUpdateEvent>('acp://update', (e) => dispatchUpdate(e.payload)),
        listen<AcpPermissionEvent>('acp://permission', (e) => dispatchPermission(e.payload)),
        listen<AcpStatusEvent>('acp://status', (e) => dispatchStatus(e.payload)),
      ])
      unlistenAll = [u1, u2, u3]
    })()
  }
  return initPromise
}

/** 释放订阅（测试/热更新用；正常运行期不卸载） */
export function disposeAcpListeners(): void {
  for (const u of unlistenAll ?? []) u()
  unlistenAll = null
  initPromise = null
}

export const acpService = {
  /** 启动 ACP 进程（幂等）：spawn + initialize 握手 */
  async start(): Promise<AcpStartResult> {
    if (!isTauri()) return acpMock.start()
    return invoke<AcpStartResult>('acp_start')
  },

  /** 查询进程状态（ready / dead / stopped + 原因） */
  async status(): Promise<AcpStatusEvent & { agentInfo?: { name: string; version: string } | null }> {
    if (!isTauri()) return acpMock.status()
    return invoke('acp_status')
  },

  /** 新建 ACP 会话（首调 7.6-8.9s，Rust 侧 30s 超时） */
  async sessionNew(cwd: string): Promise<AcpSessionResult> {
    if (!isTauri()) return acpMock.sessionNew(cwd)
    return invoke<AcpSessionResult>('acp_session_new', { cwd })
  },

  /** 一次性探针：throwaway 进程跑 initialize + session/new 取 configOptions
   * （模型列表），供会话打开/手动刷新时填充模型缓存。失败不影响会话。 */
  async probeOptions(cwd?: string): Promise<AcpConfigOption[] | null> {
    const raw = !isTauri()
      ? await acpMock.probeOptions(cwd)
      : await invoke('acp_probe_options', { cwd })
    return extractConfigOptions(raw)
  },

  /** 重载 ACP 会话（load 失败由调用方走「新建+已重连」路径，design §4） */
  async sessionLoad(acpSessionId: string, cwd: string): Promise<AcpSessionResult> {
    if (!isTauri()) return acpMock.sessionLoad(acpSessionId, cwd)
    return invoke<AcpSessionResult>('acp_session_load', { acpSessionId, cwd })
  },

  /** 发送 prompt（长请求：流式内容走 acp://update 事件，turn 结束才 resolve） */
  async prompt(sessionId: string, text: string): Promise<AcpPromptResult> {
    if (!isTauri()) return acpMock.prompt(sessionId, text)
    return invoke<AcpPromptResult>('acp_prompt', { sessionId, text })
  },

  /** 停止：session/cancel notification（无响应；pending permission 由 Rust 回 cancelled） */
  async cancel(sessionId: string): Promise<void> {
    if (!isTauri()) return acpMock.cancel(sessionId)
    await invoke('acp_cancel', { sessionId })
  },

  /** 切换会话配置（model/mode）→ 返回归一化后的全量 configOptions（协议 §8） */
  async setConfig(
    sessionId: string,
    configId: string,
    value: string,
  ): Promise<AcpConfigOption[] | null> {
    const raw = !isTauri()
      ? await acpMock.setConfig(sessionId, configId, value)
      : await invoke('acp_set_config', { sessionId, configId, value })
    return extractConfigOptions(raw)
  },

  /** 答复权限请求；optionId 为 null = cancelled 语义 */
  async respondPermission(requestId: string, optionId: string | null): Promise<void> {
    if (!isTauri()) return acpMock.respondPermission(requestId, optionId)
    await invoke('acp_respond_permission', { requestId, optionId })
  },

  /** 按 provider/model 解析上下文窗口（limit.context）：用户 opencode 配置 → models.dev
   * 缓存目录。查不到返回 null（调用方保持既有分母，不臆造）。纯文件读取，不涉 ACP 进程。 */
  async modelContextWindow(provider: string, model: string): Promise<number | null> {
    if (!isTauri()) return acpMock.modelContextWindow(provider, model)
    return invoke<number | null>('agent_model_context_window', { provider, model })
  },
}
