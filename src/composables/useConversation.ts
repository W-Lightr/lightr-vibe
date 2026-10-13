// 会话生成控制（ACP 消费面，design §3）：发送门禁、ACP update → store 映射、
// 停止、turn 结束落盘。浏览器 mock 通道由 acpMock 翻译为同形状 update，
// 本文件只有一套消费逻辑（design D5）。

import { useRouter } from 'vue-router'
import { useMessage } from 'naive-ui'
import type {
  AcpConfigOption,
  AcpToolCallEvent,
  AcpToolCallStatus,
  AcpToolCallUpdate,
  AcpUpdate,
  AcpUpdateEvent,
} from '../types/acp'
import type { MessageErrorKind, ToolCall } from '../types/session'
import { acpService } from '../services/acpService'
import { onUpdate, onStatus } from '../services/acpEvents'
import { useSessionStore } from '../stores/session'
import { useWorkspaceStore } from '../stores/workspace'
import { useAgentStore } from '../stores/agent'
import { useAcpPermission } from './useAcpPermission'
import { isTauri } from '../utils/isTauri'
import { tmpSessionDirSync } from '../utils/appData'

/** 本进程内已确认可用的 ACP 会话（sessionNew 建 / sessionLoad 载）；
 * 重启后为空 → 首次发送走 load-or-new 重连路径（design §4） */
const liveAcpSessions = new Set<string>()

/** 正在执行 session/load 的 ACP 会话：load 响应前 agent 会以 session/update
 * 重放全部历史（协议 §9），本地落盘历史为准（design §4），重放事件一律忽略，
 * 否则历史消息会重复入库 */
const replayingAcpSessions = new Set<string>()

/** toolCallId → 宿主消息（tool_call_update 无 messageId，需自持映射做累积替换）；
 * sessionId 为本地会话 id，跨会话并发时按会话精确清理 */
const toolCallHosts = new Map<string, { sessionId: string; hostId: string }>()

/** 当前 turn 的会话级状态（turnId 用于消息分组与停止标记） */
const turns = new Map<string, { turnId: string }>()

// ── 模型列表缓存（按 agent 键；本项目实际单 agent=opencode，键位预留扩展） ──
// 语义对齐 codeg 的 per-agent selectors cache：进程内缓存探针结果，
// 会话打开即可渲染真实模型列表，无需等首次发送。
const MODEL_AGENT_KEY = 'opencode'
const modelOptionsCache = new Map<string, AcpConfigOption[]>()
/** 并发探针守卫：连点「重新获取模型」不扇出多个子进程 */
let probeInFlight: Promise<AcpConfigOption[] | null> | null = null

/** 读取已缓存的配置项（无则 null） */
export function getCachedModelOptions(): AcpConfigOption[] | null {
  return modelOptionsCache.get(MODEL_AGENT_KEY) ?? null
}

/**
 * 真实探针刷新模型列表：调 acp_probe_options（或 mock）→ 写缓存；
 * 传入 sessionId 时同步应用到该会话的 configOptions（下拉即时更新）。
 * 并发调用共享同一 in-flight promise。
 */
export async function refreshModelOptions(
  sessionId?: string,
): Promise<AcpConfigOption[] | null> {
  if (!probeInFlight) {
    probeInFlight = (async () => {
      try {
        const session = sessionId ? useSessionStore().get(sessionId) : null
        const options = await acpService.probeOptions(session?.cwd)
        if (options && options.length) {
          modelOptionsCache.set(MODEL_AGENT_KEY, options)
          return options
        }
        return null
      } finally {
        probeInFlight = null
      }
    })()
  }
  const options = await probeInFlight
  if (options && sessionId) {
    const session = useSessionStore().get(sessionId)
    if (session) {
      session.configOptions = options
      // 同步 session.model 到探针返回的模型 currentValue（下拉显示一致）
      const modelOpt = options.find((o) => o.id === 'model' || o.category === 'model')
      if (typeof modelOpt?.currentValue === 'string') session.model = modelOpt.currentValue
    }
  }
  return options
}

/**
 * 会话打开时的后台便利探针：缓存/会话尚无模型项时才跑，失败静默（不产错误卡，
 * 不阻塞 UI）。缓存命中时即时把缓存灌入会话，使下拉立即有真值。
 */
export async function ensureModelOptions(sessionId: string): Promise<void> {
  const session = useSessionStore().get(sessionId)
  if (!session) return
  const hasModel = session.configOptions?.some(
    (o) => o.id === 'model' || o.category === 'model',
  )
  if (hasModel) return
  const cached = getCachedModelOptions()
  if (cached) {
    session.configOptions = cached
    const modelOpt = cached.find((o) => o.id === 'model' || o.category === 'model')
    if (typeof modelOpt?.currentValue === 'string') session.model = modelOpt.currentValue
    return
  }
  try {
    await refreshModelOptions(sessionId)
  } catch {
    // 后台便利探针：失败静默（模型下拉继续用 MODELS 兜底）
  }
}

function mapToolStatus(status?: AcpToolCallStatus): ToolCall['status'] {
  switch (status) {
    case 'completed':
      return 'done'
    case 'failed':
      return 'error'
    default:
      return 'running'
  }
}

/** tool_call_update 的 content（ToolCallContent[]）→ 纯文本摘要（累积替换语义） */
function flattenToolContent(content: unknown): string {
  if (!Array.isArray(content)) return ''
  const parts: string[] = []
  for (const item of content) {
    if (typeof item === 'object' && item !== null) {
      const rec = item as { content?: { type?: string; text?: string }; type?: string; text?: string }
      const inner = rec.content ?? rec
      if (inner.type === 'text' && typeof inner.text === 'string') parts.push(inner.text)
    }
  }
  return parts.join('\n')
}

function summarizeRawInput(rawInput: unknown): string {
  if (rawInput === undefined || rawInput === null) return ''
  if (typeof rawInput === 'string') return rawInput
  try {
    return JSON.stringify(rawInput)
  } catch {
    return String(rawInput)
  }
}

/** Rust 错误串 → 错误卡类别（acp.rs 约定前缀） */
export function classifyAcpError(err: unknown): { kind: MessageErrorKind; text: string } {
  const text = err instanceof Error ? err.message : String(err)
  if (text.includes('acp-dead')) {
    return { kind: 'dead', text: 'Agent 进程已退出，请重新启动后继续对话' }
  }
  if (text.includes('timeout:')) {
    return { kind: 'timeout', text: `Agent 响应超时（${text}），请稍后重试` }
  }
  if (text.includes('acp:-32000') || text.includes('Authentication required')) {
    return { kind: 'auth', text: '当前模型需要凭据：请在终端运行 `opencode auth login` 后重试' }
  }
  return { kind: 'generic', text: `生成失败：${text}` }
}

let updateSubscribed = false

/** 模块级订阅（一次）：acp://update → store；acp://status dead → 活动会话错误卡 */
function ensureSubscribed(): void {
  if (updateSubscribed) return
  updateSubscribed = true

  onUpdate((event: AcpUpdateEvent) => applyUpdate(event))

  onStatus((event) => {
    if (event.status !== 'dead') return
    liveAcpSessions.clear()
    replayingAcpSessions.clear()
    // PRD R1：进程死亡 → 已绑定 ACP 的会话标 error 态（检视器徽章）。
    // 生成中的会话其错误卡/generating 复位由 send() 的 catch/finally 收敛
    // （Rust 侧清 pending → 挂起 prompt 必以 acp-dead reject），避免双份错误卡；
    // 下次发送时 ensureAcpSession 自动重启进程并 load-or-new（design D1 一键恢复）
    const sessionStore = useSessionStore()
    for (const s of sessionStore.sessions) {
      if (s.acpSessionId && s.status !== 'generating') sessionStore.setStatus(s.id, 'error')
    }
  })
}

function applyUpdate(event: AcpUpdateEvent): void {
  const sessionStore = useSessionStore()
  const acpSessionId = event.sessionId
  const update = event.update
  // session/load 响应前的历史重放（协议 §9）：本地落盘历史为准（design §4），全部忽略
  if (replayingAcpSessions.has(acpSessionId)) return
  // 事件携带的是 ACP 侧 sessionId（opencode ses_xxx），必须先映射回本地会话
  const session = sessionStore.findByAcpSessionId(acpSessionId)
  if (!session) return
  const sessionId = session.id
  const turnId = turns.get(sessionId)?.turnId

  switch (update.sessionUpdate) {
    case 'agent_message_chunk':
    case 'agent_thought_chunk':
    case 'user_message_chunk': {
      // user_message_chunk 本地历史已含（发送时即入库），忽略防重复
      if (update.sessionUpdate === 'user_message_chunk') return
      const text = update.content?.text ?? ''
      const type = update.sessionUpdate === 'agent_thought_chunk' ? 'thinking' : 'text'
      // 兜底 id 带 turnId：无 messageId 的 agent 跨 turn 不与上一条 done 消息碰撞
      const messageId = update.messageId ?? `${type}-current-${turnId ?? 'live'}`
      let message = session?.messages.find((m) => m.id === messageId)
      if (!message) {
        message = sessionStore.pushMessage(sessionId, {
          id: messageId,
          role: 'assistant',
          type,
          content: '',
          status: 'streaming',
          turnId,
        })
      }
      if (message.status === 'done' || message.status === 'stopped') return
      sessionStore.appendContent(sessionId, message.id, text)
      break
    }
    case 'tool_call':
    case 'tool_call_update': {
      applyToolCall(sessionId, update, turnId)
      break
    }
    case 'usage_update': {
      if (typeof update.used === 'number') {
        sessionStore.setContextUsage(sessionId, update.used, update.size)
      }
      break
    }
    case 'config_option_update': {
      if (session && Array.isArray(update.configOptions)) {
        session.configOptions = update.configOptions
      }
      break
    }
    // plan / available_commands_update / current_mode_update / session_info_update：
    // 本期面板不展示（design §3），忽略
    default:
      break
  }
}

function applyToolCall(sessionId: string, update: AcpToolCallEvent, turnId: string | undefined): void {
  const sessionStore = useSessionStore()
  const toolCallId = update.toolCallId
  if (!toolCallId) return
  const host = toolCallHosts.get(toolCallId)

  if (!host) {
    // 首个 tool_call：新建宿主消息承载工具卡（同 turn 多工具各占一条）
    const tc: ToolCall = {
      id: toolCallId,
      name: update.title ?? toolCallId,
      input: summarizeRawInput(update.rawInput),
      status: mapToolStatus(update.status),
    }
    const result = flattenToolContent(update.content)
    if (result) tc.result = result
    const created = sessionStore.pushMessage(sessionId, {
      role: 'assistant',
      type: 'text',
      content: '',
      status: 'streaming',
      turnId,
      toolCalls: [tc],
    })
    toolCallHosts.set(toolCallId, { sessionId, hostId: created.id })
    return
  }

  // tool_call_update：累积替换（probe §3：content 每次带全量，覆盖不 append）
  const patch: Partial<ToolCall> = {}
  if (update.title !== undefined) patch.name = update.title
  if (update.rawInput !== undefined) patch.input = summarizeRawInput(update.rawInput)
  if (update.status !== undefined) patch.status = mapToolStatus(update.status)
  const content = flattenToolContent(update.content)
  if (content) patch.result = content
  sessionStore.patchToolCall(host.sessionId, host.hostId, toolCallId, patch)
}

/** configOptions.select 的取值集合（扁平与分组两种形态，协议 §8） */
function collectOptionValues(options: AcpConfigOption['options']): string[] {
  const values: string[] = []
  for (const opt of options ?? []) {
    if ('value' in opt && typeof opt.value === 'string') values.push(opt.value)
    else if ('options' in opt) {
      for (const sub of opt.options ?? []) {
        if (typeof sub.value === 'string') values.push(sub.value)
      }
    }
  }
  return values
}

/** 确保 ACP 会话可用：live 直接复用；否则 load → 失败 new + 标题「（已重连）」 */
async function ensureAcpSession(sessionId: string): Promise<string> {
  const sessionStore = useSessionStore()
  const session = sessionStore.get(sessionId)
  if (!session) throw new Error('acp-dead: 会话不存在')

  await acpService.start()
  const cwd = session.cwd ?? tmpSessionDirSync()

  if (session.acpSessionId && liveAcpSessions.has(session.acpSessionId)) {
    return session.acpSessionId
  }
  if (session.acpSessionId) {
    // load 响应前 agent 会以 session/update 重放全部历史（协议 §9），
    // 本地落盘历史为准（design §4）：重放期间忽略该会话的 update，响应到达即恢复
    const replayId = session.acpSessionId
    replayingAcpSessions.add(replayId)
    try {
      const loaded = await acpService.sessionLoad(replayId, cwd)
      session.acpSessionId = loaded.sessionId
      if (Array.isArray(loaded.configOptions) && loaded.configOptions.length) {
        session.configOptions = loaded.configOptions
      }
      liveAcpSessions.add(loaded.sessionId)
      return loaded.sessionId
    } catch {
      // load 失败（进程重启后旧会话不可达等）→ 新建并保留本地历史（design §4）
      if (!session.title.endsWith('（已重连）')) session.title += '（已重连）'
    } finally {
      replayingAcpSessions.delete(replayId)
    }
  }
  const created = await acpService.sessionNew(cwd)
  session.acpSessionId = created.sessionId
  if (Array.isArray(created.configOptions) && created.configOptions.length) {
    session.configOptions = created.configOptions
    const modelOpt = created.configOptions.find((o) => o.id === 'model' || o.category === 'model')
    if (modelOpt) {
      // workspace.selectedModel 作为新建会话默认值：仅当它在 ACP 模型列表中才应用
      // （PRD R2；静态 MODELS 标签可能不是合法模型 id，盲发会 -32602）
      const wanted = session.model
      const available = collectOptionValues(modelOpt.options)
      let applied = typeof modelOpt.currentValue === 'string' ? modelOpt.currentValue : wanted
      if (wanted && available.includes(wanted) && applied !== wanted) {
        try {
          await acpService.setConfig(created.sessionId, modelOpt.id, wanted)
          applied = wanted
        } catch {
          // 应用失败退回 Agent 当前模型，不阻断发送
        }
      }
      if (applied) session.model = applied
    }
    const modeOpt = created.configOptions.find((o) => o.id === 'mode' || o.category === 'mode')
    if (modeOpt && typeof modeOpt.currentValue === 'string') session.mode = modeOpt.currentValue
  }
  liveAcpSessions.add(created.sessionId)
  return created.sessionId
}

export function useConversation() {
  const sessionStore = useSessionStore()
  const workspace = useWorkspaceStore()
  const agentStore = useAgentStore()
  const router = useRouter()
  const message = useMessage()
  const { cancelQueued } = useAcpPermission()

  ensureSubscribed()

  /** 发送门禁（Q1）：Tauri 环境要求 installed && acpOk，否则引导跳设置页且不产生消息 */
  async function passGate(): Promise<boolean> {
    if (!isTauri()) return true
    if (!agentStore.detectResult) {
      try {
        await agentStore.detect()
      } catch {
        // 检测失败按不可用处理
      }
    }
    const d = agentStore.detectResult
    if (d && d.installed && d.acpOk) return true
    message.warning('未检测到可用 Agent，请先到设置页完成 OpenCode 配置')
    void router.push('/settings')
    return false
  }

  async function send(sessionId: string, content: string) {
    if (!content.trim() || workspace.isGenerating(sessionId)) return
    if (!(await passGate())) return

    // 懒加载兜底：索引恢复的会话首次发送前确保消息已读入（避免落盘覆盖丢历史）
    await sessionStore.ensureMessages(sessionId)

    const turnId = `turn-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`
    sessionStore.pushMessage(sessionId, { role: 'user', type: 'text', content, status: 'done', turnId })
    sessionStore.applyAutoTitle(sessionId, content)
    sessionStore.setStatus(sessionId, 'generating')
    workspace.setGenerating(sessionId, true)
    turns.set(sessionId, { turnId })

    try {
      const acpSessionId = await ensureAcpSession(sessionId)
      const result = await acpService.prompt(acpSessionId, content)
      // turn 结束：流式消息收尾（cancelled → stopped；其余 → done）
      const finalStatus = result.stopReason === 'cancelled' ? 'stopped' : 'done'
      const session = sessionStore.get(sessionId)
      for (const m of session?.messages ?? []) {
        if (m.turnId === turnId && m.status === 'streaming') {
          sessionStore.patchMessage(sessionId, m.id, { status: finalStatus })
        }
      }
      if (result.stopReason === 'refusal') {
        sessionStore.pushMessage(sessionId, {
          role: 'assistant',
          type: 'error',
          content: 'Agent 拒绝了本次请求',
          status: 'error',
          errorKind: 'generic',
          turnId,
        })
      }
      // prompt response 的 usage（opencode 扩展：inputTokens/outputTokens，缓存量可能缺省）
      if (result.usage) {
        sessionStore.addUsage(sessionId, {
          input: result.usage.inputTokens ?? 0,
          output: result.usage.outputTokens ?? 0,
          cachedRead: result.usage.cachedReadTokens ?? 0,
          cachedWrite: result.usage.cachedWriteTokens ?? 0,
        })
      }
      // turn 结束落盘（D4：流式期间不写）
      void sessionStore.persistMessages(sessionId)
    } catch (err) {
      const { kind, text } = classifyAcpError(err)
      if (kind === 'dead') liveAcpSessions.clear()
      // 超时后 agent 可能仍在跑该 turn（Rust 只放弃等待）：补一发 cancel 止血，
      // 迟到 update 因 turnId 已清不再产生悬挂的流式消息
      if (kind === 'timeout') stop(sessionId)
      // 收尾本 turn 仍在流式的消息，避免永久「打字光标」悬挂
      for (const m of sessionStore.get(sessionId)?.messages ?? []) {
        if (m.turnId === turnId && m.status === 'streaming') {
          sessionStore.patchMessage(sessionId, m.id, { status: 'stopped' })
        }
      }
      sessionStore.pushMessage(sessionId, {
        role: 'assistant',
        type: 'error',
        content: text,
        status: 'error',
        errorKind: kind,
        turnId,
      })
      sessionStore.setStatus(sessionId, 'error')
      void sessionStore.persistMessages(sessionId)
    } finally {
      for (const [tid, entry] of toolCallHosts) {
        // 只清理本会话、本 turn 的工具宿主映射（跨会话并发时不误删）
        if (entry.sessionId !== sessionId) continue
        const host = sessionStore.messageById(sessionId, entry.hostId)
        if (!host || host.turnId === turnId) toolCallHosts.delete(tid)
      }
      turns.delete(sessionId)
      workspace.setGenerating(sessionId, false)
      if (sessionStore.get(sessionId)?.status === 'generating') {
        sessionStore.setStatus(sessionId, 'idle')
      }
      sessionStore.touch(sessionId)
    }
  }

  /** 停止：session/cancel（44ms 内回 cancelled，probe §4）+ 本地 pending permission 回 cancelled */
  function stop(sessionId: string) {
    const session = sessionStore.get(sessionId)
    if (session?.acpSessionId) {
      cancelQueued(sessionId)
      void acpService.cancel(session.acpSessionId).catch(() => undefined)
    }
  }

  /** 就地写某个 configOption 的 currentValue（乐观 UI；ACP 回包未含该 option 时的兜底） */
  function applyOptionValue(
    session: { configOptions?: AcpConfigOption[] },
    configId: string,
    value: string,
  ) {
    const opt = session.configOptions?.find((o) => o.id === configId)
    if (opt) opt.currentValue = value
  }

  /** 按 provider/model 解析并写入上下文窗口分母（best-effort，失败静默）。
   * 模型 value 形如 `provider/modelId`，modelId 自身可能含 `/`（如 magpie/workbuddy/xxx），
   * 故仅按第一个 `/` 切分。set_config_option 回包不含 limit.context，须本地查 config/catalog。 */
  async function refreshContextWindow(
    session: { tokenUsage: { contextWindow: number } },
    modelValue: string,
  ): Promise<void> {
    const slash = modelValue.indexOf('/')
    if (slash <= 0 || slash === modelValue.length - 1) return
    const provider = modelValue.slice(0, slash)
    const model = modelValue.slice(slash + 1)
    try {
      const win = await acpService.modelContextWindow(provider, model)
      if (typeof win === 'number' && win > 0) session.tokenUsage.contextWindow = win
    } catch {
      // 静默：查不到就保持既有分母（usage_update.size 仍会覆盖）
    }
  }

  /** 通用会话配置切换（set_config_option）：model / effort 等 select 项共用。
   * 乐观更新：先写 in-memory currentValue（chip/标签即时反馈），成功后用 ACP 回包
   * 覆盖；失败（如需凭据模型 -32602）→ 回退旧值 + 错误卡而非静默（AC7）。
   * 未建 ACP 会话时仅做本地写入（首个 prompt 建连后 acp_set_config 才可用）。 */
  async function setConfigOption(
    sessionId: string,
    configId: string,
    value: string,
  ): Promise<void> {
    const session = sessionStore.get(sessionId)
    if (!session) return
    const opt = session.configOptions?.find((o) => o.id === configId)
    const prevValue = opt?.currentValue
    // 乐观写入（model 额外同步 session.model 字段）
    applyOptionValue(session, configId, value)
    if (configId === 'model') session.model = value

    // 切换模型即刷新上下文窗口分母（不等下一条 usage_update；best-effort 不阻塞）
    if (configId === 'model') void refreshContextWindow(session, value)

    if (!session.acpSessionId || !liveAcpSessions.has(session.acpSessionId)) {
      // 未建连：本地生效即可（首个 prompt 走 session/new 时再以 configOptions 为准）
      sessionStore.touch(sessionId)
      return
    }
    try {
      const opts = await acpService.setConfig(session.acpSessionId, configId, value)
      if (configId === 'model') session.model = value
      if (opts) {
        session.configOptions = opts
      } else {
        // 回包未含 options：保留乐观值
        applyOptionValue(session, configId, value)
      }
      sessionStore.touch(sessionId)
    } catch (err) {
      // 失败：回退到旧值（model 同步回退 session.model）
      if (typeof prevValue === 'string') {
        applyOptionValue(session, configId, prevValue)
        if (configId === 'model') session.model = prevValue
      }
      const { kind, text } = classifyAcpError(err)
      const raw = err instanceof Error ? err.message : String(err)
      sessionStore.pushMessage(sessionId, {
        role: 'assistant',
        type: 'error',
        // generic 分支的 text 自带「生成失败：」前缀，配置切换场景改用原始错误串
        content: kind === 'generic' ? `切换失败：${raw}` : text,
        status: 'error',
        errorKind: kind,
      })
    }
  }

  /** 切换会话模型：set_config_option('model') 的薄封装（保留既有调用方） */
  async function setModel(sessionId: string, modelValue: string): Promise<void> {
    return setConfigOption(sessionId, 'model', modelValue)
  }

  /** 从此分叉：委托 session store 复制历史到新会话（不调 ACP，acpSessionId 懒建）。
   * 返回新会话供调用方打开标签；源会话不存在时返回 null。 */
  function fork(sessionId: string, messageId: string) {
    return sessionStore.forkSession(sessionId, messageId)
  }

  /** 重新生成：移除该回合全部消息（含用户消息，send 会重新入库避免重复气泡），重发 */
  function regenerate(sessionId: string, assistantMessageId: string) {
    const session = sessionStore.get(sessionId)
    const target = session?.messages.find((m) => m.id === assistantMessageId)
    if (!session || !target?.turnId || workspace.isGenerating(sessionId)) return
    const turnId = target.turnId
    const userMsg = [...session.messages].reverse().find((m) => m.turnId === turnId && m.role === 'user')
    session.messages = session.messages.filter((m) => m.turnId !== turnId)
    if (userMsg) void send(sessionId, userMsg.content)
  }

  return { send, stop, regenerate, setModel, setConfigOption, fork }
}

/** ACP update 类型收窄辅助（组件/测试可复用） */
export type { AcpUpdate, AcpToolCallUpdate }
