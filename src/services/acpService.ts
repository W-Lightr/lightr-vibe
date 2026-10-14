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
  AcpQuestionEvent,
  AcpSessionResult,
  AcpStartResult,
  AcpStatusEvent,
  AcpUpdateEvent,
} from '../types/acp'
import { isTauri } from '../utils/isTauri'
import { dispatchPermission, dispatchQuestion, dispatchStatus, dispatchUpdate } from './acpEvents'
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
      const [u1, u2, u3, u4] = await Promise.all([
        listen<AcpUpdateEvent>('acp://update', (e) => dispatchUpdate(e.payload)),
        listen<AcpPermissionEvent>('acp://permission', (e) => {
          console.debug('[acp-listener] acp://permission', e.payload?.connId, e.payload?.toolCall?.kind)
          dispatchPermission(e.payload)
        }),
        listen<AcpQuestionEvent>('acp://question', (e) => {
          console.debug('[acp-listener] acp://question', e.payload?.connId, e.payload?.questions?.length)
          dispatchQuestion(e.payload)
        }),
        listen<AcpStatusEvent>('acp://status', (e) => dispatchStatus(e.payload)),
      ])
      unlistenAll = [u1, u2, u3, u4]
      console.debug('[acp-listener] listeners ready')
    })().catch((err) => {
      // 初始化失败（webview 未就绪等）：清空缓存以便下次重试，不让一个 reject 永久卡死监听
      console.error('[acp-listener] init failed, will retry on next call', err)
      initPromise = null
    })
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
  /** 建立/复用某会话的 ACP 连接（幂等）：为该 connId(本地会话 id) spawn 独立进程 +
   *  initialize 握手（design §8.1 每会话一进程）。engineId 为该会话绑定的引擎 wire。 */
  async connect(sessionId: string, engineId: string): Promise<AcpStartResult> {
    if (!isTauri()) return acpMock.connect(sessionId, engineId)
    // 保证事件监听已就绪（避免 main.ts 早起失败后永久无订阅）：connect 在任何 prompt 之前
    await initAcpListeners()
    return invoke<AcpStartResult>('acp_connect', { sessionId, engineId })
  },

  /** 断开某会话连接（杀该会话进程）：关标签/删除会话时调用；幂等（不存在 no-op） */
  async disconnect(sessionId: string): Promise<void> {
    if (!isTauri()) return acpMock.disconnect(sessionId)
    await invoke('acp_disconnect', { sessionId })
  },

  /** 查询某会话连接状态（ready / dead / stopped + 原因） */
  async status(connId: string): Promise<AcpStatusEvent & { agentInfo?: { name: string; version: string } | null }> {
    if (!isTauri()) return acpMock.status(connId)
    return invoke('acp_status', { connId })
  },

  /** 新建 ACP 会话（首调 7.6-8.9s，Rust 侧 30s 超时） */
  async sessionNew(connId: string, cwd: string): Promise<AcpSessionResult> {
    if (!isTauri()) return acpMock.sessionNew(connId, cwd)
    return invoke<AcpSessionResult>('acp_session_new', { connId, cwd })
  },

  /** 一次性探针：throwaway 进程跑 initialize + session/new 取 configOptions
   * （模型列表），供会话打开/手动刷新时填充模型缓存。失败不影响会话。
   * engineId 缺省由 Rust 侧回退 opencode。 */
  async probeOptions(cwd?: string, engineId?: string): Promise<AcpConfigOption[] | null> {
    const raw = !isTauri()
      ? await acpMock.probeOptions(cwd, engineId)
      : await invoke('acp_probe_options', { cwd, engineId })
    return extractConfigOptions(raw)
  },

  /** 重载 ACP 会话（load 失败由调用方走「新建+已重连」路径，design §4） */
  async sessionLoad(connId: string, acpSessionId: string, cwd: string): Promise<AcpSessionResult> {
    if (!isTauri()) return acpMock.sessionLoad(connId, acpSessionId, cwd)
    return invoke<AcpSessionResult>('acp_session_load', { connId, acpSessionId, cwd })
  },

  /** 发送 prompt（长请求：流式内容走 acp://update 事件，turn 结束才 resolve）。
   * connId = 本地会话 id（定位进程）；sessionId = agent-side id（方法参数）。 */
  async prompt(connId: string, sessionId: string, text: string): Promise<AcpPromptResult> {
    if (!isTauri()) return acpMock.prompt(connId, sessionId, text)
    return invoke<AcpPromptResult>('acp_prompt', { connId, sessionId, text })
  },

  /** 停止：session/cancel notification（无响应；pending permission 由 Rust 回 cancelled） */
  async cancel(connId: string, sessionId: string): Promise<void> {
    if (!isTauri()) return acpMock.cancel(connId, sessionId)
    await invoke('acp_cancel', { connId, sessionId })
  },

  /** 关闭 ACP 会话（session/close）：释放 agent 侧会话资源，连接（进程）保持存活。
   * 连接销毁走 acp_disconnect。sessionId 为 ACP 侧 id（ses_xxx）。 */
  async closeSession(connId: string, sessionId: string): Promise<void> {
    if (!isTauri()) return acpMock.closeSession(connId, sessionId)
    await invoke('acp_session_close', { connId, sessionId })
  },

  /** 切换会话配置（model/mode）→ 返回归一化后的全量 configOptions（协议 §8） */
  async setConfig(
    connId: string,
    sessionId: string,
    configId: string,
    value: string,
  ): Promise<AcpConfigOption[] | null> {
    const raw = !isTauri()
      ? await acpMock.setConfig(connId, sessionId, configId, value)
      : await invoke('acp_set_config', { connId, sessionId, configId, value })
    return extractConfigOptions(raw)
  },

  /** 答复权限请求；optionId 为 null = cancelled 语义 */
  async respondPermission(
    connId: string,
    requestId: string,
    optionId: string | null,
  ): Promise<void> {
    if (!isTauri()) return acpMock.respondPermission(connId, requestId, optionId)
    await invoke('acp_respond_permission', { connId, requestId, optionId })
  },

  /** 答复 elicitation 提问（Questions 型）：answers 为 {fieldId,values} 列表，
   *  declined=true 表示跳过/拒答。Rust 侧构造 accept/decline 响应回写 agent。 */
  async answerElicitation(
    connId: string,
    requestId: string,
    answers: { fieldId: string; values: string[] }[],
    declined: boolean,
  ): Promise<void> {
    if (!isTauri()) return acpMock.answerElicitation(connId, requestId, answers, declined)
    await invoke('acp_answer_elicitation', { connId, requestId, answers, declined })
  },

  /** 按 provider/model 解析上下文窗口（limit.context），策略来自引擎声明。查不到返回 null
   * （调用方保持既有分母，不臆造）。纯文件读取，不涉 ACP 进程。engineId 缺省回退 opencode。 */
  async modelContextWindow(provider: string, model: string, engineId?: string): Promise<number | null> {
    if (!isTauri()) return acpMock.modelContextWindow(provider, model, engineId)
    return invoke<number | null>('agent_model_context_window', { provider, model, engineId })
  },
}
