// ACP 浏览器 mock 通道：把现有 conversationService 的 MockEvent 流翻译为
// ACP update / permission 形状（design D5），使 useConversation 只有一套消费逻辑。
// 与 acpService.ts 返回结构一致（tauri-bridge spec §2 双通道约定）。

import type {
  AcpConfigOption,
  AcpPermissionEvent,
  AcpPromptResult,
  AcpSessionResult,
  AcpStartResult,
  AcpStatusEvent,
} from '../types/acp'
import { createId } from '../utils/id'
import { dispatchPermission, dispatchUpdate } from './acpEvents'
import { conversationService } from './conversationService'

/** mock 会话级配置项（对应 probe §2 实测形态：model + mode select） */
function mockConfigOptions(): AcpConfigOption[] {
  return [
    {
      id: 'model',
      name: 'Model',
      category: 'model',
      type: 'select',
      currentValue: 'mock/gpt-mini',
      options: [
        { value: 'mock/gpt-mini', name: 'Mock GPT Mini' },
        { value: 'mock/gpt-pro', name: 'Mock GPT Pro' },
        { value: 'mock/claude-lite', name: 'Mock Claude Lite' },
      ],
    },
    {
      id: 'effort',
      name: 'Reasoning Effort',
      category: 'effort',
      type: 'select',
      currentValue: 'medium',
      options: [
        { value: 'none', name: 'None' },
        { value: 'low', name: 'Low' },
        { value: 'medium', name: 'Medium' },
        { value: 'high', name: 'High' },
        { value: 'max', name: 'Max' },
      ],
    },
    {
      id: 'mode',
      name: 'Session Mode',
      category: 'mode',
      type: 'select',
      currentValue: 'build',
      options: [
        { value: 'build', name: 'build', description: '默认 Agent，按权限配置执行工具' },
        { value: 'plan', name: 'plan', description: '计划模式，禁用所有编辑工具' },
      ],
    },
  ]
}

/** 已「连接」的 mock 会话（connId = 本地会话 id）：connect/disconnect 幂等（design §8.1） */
const connectedSessions = new Set<string>()

/** 每个 mock 会话的 AbortController（按 connId 键；每会话一进程语义） */
const activeControllers = new Map<string, AbortController>()
const pendingPermissions = new Map<string, () => void>()
let permSeq = 0

/** 模拟一次 tool_call 的权限请求（对应真实 session/request_permission） */
function simulatePermission(
  connId: string,
  sessionId: string,
  toolCallId: string,
  title: string,
  kind: 'read' | 'edit' | 'execute' = 'execute',
): void {
  const requestId = `mock-perm-${++permSeq}`
  const event: AcpPermissionEvent = {
    connId,
    requestId,
    sessionId,
    toolCall: {
      toolCallId,
      title,
      kind,
      status: 'pending',
      rawInput: { command: `echo ${title}` },
    },
    options: [
      { optionId: 'allow_once', name: '允许一次', kind: 'allow_once' },
      { optionId: 'allow_always', name: '本会话总是允许', kind: 'allow_always' },
      { optionId: 'reject_once', name: '拒绝', kind: 'reject_once' },
    ],
  }
  pendingPermissions.set(requestId, () => pendingPermissions.delete(requestId))
  dispatchPermission(event)
}

export const acpMock = {
  async connect(sessionId: string, _engineId?: string): Promise<AcpStartResult> {
    connectedSessions.add(sessionId)
    return {
      status: 'ready',
      protocolVersion: 1,
      agentInfo: { name: 'OpenCode (mock)', version: 'mock' },
    }
  },

  async disconnect(sessionId: string): Promise<void> {
    connectedSessions.delete(sessionId)
    activeControllers.get(sessionId)?.abort()
  },

  async status(connId: string): Promise<AcpStatusEvent & { agentInfo?: { name: string; version: string } | null }> {
    if (connectedSessions.has(connId)) {
      return { connId, status: 'ready', agentInfo: { name: 'OpenCode (mock)', version: 'mock' } }
    }
    return { connId, status: 'stopped' }
  },

  async sessionNew(_connId: string, _cwd: string): Promise<AcpSessionResult> {
    return { sessionId: createId('ses-mock'), configOptions: mockConfigOptions() }
  },

  async sessionLoad(_connId: string, acpSessionId: string, _cwd: string): Promise<AcpSessionResult> {
    return { sessionId: acpSessionId, configOptions: mockConfigOptions() }
  },

  /** 一次性探针（浏览器）：复用 mock 配置项生成器，形状与 acp_probe_options 一致 */
  async probeOptions(_cwd?: string, _engineId?: string): Promise<{ configOptions: AcpConfigOption[] }> {
    return { configOptions: mockConfigOptions() }
  },

  /**
   * 模拟一次完整 turn：思考 → 工具调用（含权限请求）→ 流式回复 → usage。
   * conversationService.send 的 MockEvent 流在此翻译为 dispatchUpdate 的 ACP update 形状；
   * cancel() 通过 AbortController 中断，按真实语义返回 stopReason:'cancelled'（probe §4）。
   * 工具形态随输入：含「写」→ write_file（改写类，plan 模式会被拒绝），
   * 否则 read_file（只读，plan 模式放行）——保证浏览器 dev 可演示权限三模式（AC8）。
   * connId = 本地会话 id（每会话一进程语义）；sessionId = agent-side id（消息归属）。
   */
  async prompt(connId: string, sessionId: string, text: string): Promise<AcpPromptResult> {
    const controller = new AbortController()
    activeControllers.set(connId, controller)
    let outputTokens = 0
    let permissionTriggered = false
    const isWrite = text.includes('写')

    try {
      await conversationService.send({
        sessionId,
        content: text,
        model: 'mock',
        signal: controller.signal,
        onDelta: (messageId, delta) => {
          outputTokens += delta.length
          dispatchUpdate({
            connId,
            sessionId,
            update: {
              sessionUpdate: 'agent_message_chunk',
              messageId,
              content: { type: 'text', text: delta },
            },
          })
        },
        onEvent: (event) => {
          switch (event.kind) {
            case 'thinking-start':
              dispatchUpdate({
                connId,
                sessionId,
                update: {
                  sessionUpdate: 'agent_thought_chunk',
                  messageId: event.messageId,
                  content: { type: 'text', text: '' },
                },
              })
              break
            case 'thinking-delta':
              dispatchUpdate({
                connId,
                sessionId,
                update: {
                  sessionUpdate: 'agent_thought_chunk',
                  messageId: event.messageId,
                  content: { type: 'text', text: event.delta },
                },
              })
              break
            case 'thinking-end':
              // 思考流结束无专用 update 变体；真实 ACP 也不发终止标记，
              // 由 useConversation 在收到后续 chunk 类型切换时隐式收尾
              break
            case 'tool-start': {
              const tc = event.toolCall
              // 输入含「写」时演示改写类工具（kind=edit → plan 模式拒绝），否则只读
              const name = isWrite ? 'write_file' : tc.name
              const kind = isWrite ? 'edit' : 'read'
              dispatchUpdate({
                connId,
                sessionId,
                update: {
                  sessionUpdate: 'tool_call',
                  toolCallId: tc.id,
                  title: name,
                  kind,
                  status: 'in_progress',
                  rawInput: { path: tc.input },
                },
              })
              if (!permissionTriggered) {
                permissionTriggered = true
                simulatePermission(connId, sessionId, tc.id, name, kind)
              }
              break
            }
            case 'tool-end':
              dispatchUpdate({
                connId,
                sessionId,
                update: {
                  sessionUpdate: 'tool_call_update',
                  toolCallId: event.toolCallId,
                  status: 'completed',
                  content: [{ type: 'content', content: { type: 'text', text: event.result } }],
                },
              })
              break
            case 'reply-start':
              dispatchUpdate({
                connId,
                sessionId,
                update: {
                  sessionUpdate: 'agent_message_chunk',
                  messageId: event.messageId,
                  content: { type: 'text', text: '' },
                },
              })
              break
            case 'reply-end':
              dispatchUpdate({
                connId,
                sessionId,
                update: {
                  sessionUpdate: 'usage_update',
                  // mock：used=上下文占用（≈本次输入的 prompt token，不含 output）；
                  // size=窗口容量。真实 opencode 的 used=input+cache.read+cache.write
                  used: event.usage.input,
                  size: 200_000,
                },
              })
              break
            case 'error':
              // 真实 ACP 中错误经 prompt response 的 error/-32xxx 返回（非 update 变体）；
              // mock 侧由 conversationService.send throw → acpMock.prompt reject 走同一路径
              break
          }
        },
      })
      const inputTokens = Math.round(text.length * 1.6) + 320
      return {
        stopReason: 'end_turn',
        usage: { inputTokens, outputTokens, totalTokens: inputTokens + outputTokens },
      }
    } catch (err) {
      if ((err as DOMException)?.name === 'AbortError') {
        // 真实 ACP：cancel 后 prompt 请求正常响应 stopReason:'cancelled'（非异常）
        return { stopReason: 'cancelled', usage: null }
      }
      throw err
    } finally {
      activeControllers.delete(connId)
    }
  },

  async cancel(connId: string, _sessionId: string): Promise<void> {
    activeControllers.get(connId)?.abort()
  },

  /** 关闭会话（session/close）：mock 无 agent 侧会话注册表，no-op */
  async closeSession(_connId: string, _sessionId: string): Promise<void> {
    // mock 通道无需释放任何资源
  },

  async setConfig(
    _connId: string,
    _sessionId: string,
    configId: string,
    value: string,
  ): Promise<{ configOptions: AcpConfigOption[] }> {
    const options = mockConfigOptions()
    for (const opt of options) {
      if (opt.id === configId) opt.currentValue = value
    }
    return { configOptions: options }
  },

  async respondPermission(_connId: string, requestId: string, _optionId: string | null): Promise<void> {
    pendingPermissions.get(requestId)?.()
  },

  /** 浏览器 mock：固定的模型上下文窗口表；未命中返回 null */
  modelContextWindow(provider: string, model: string, _engineId?: string): number | null {
    const key = `${provider}/${model}`
    const map: Record<string, number> = {
      'mock/gpt-mini': 128_000,
      'mock/gpt-pro': 200_000,
      'mock/claude-lite': 200_000,
    }
    return map[key] ?? null
  },
}
