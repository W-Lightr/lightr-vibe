// 会话生成控制（ACP 消费面，design §3）：发送门禁、ACP update → store 映射、
// 停止、turn 结束落盘。浏览器 mock 通道由 acpMock 翻译为同形状 update，
// 本文件只有一套消费逻辑（design D5）。
// 每会话独立进程（design §8.3）：connId = 本地会话 id，事件按 connId 路由；
// 关标签 = 杀该会话进程（closeConnection → acp_disconnect）。

import { useRouter } from 'vue-router'
import { useMessage } from 'naive-ui'
import { ref } from 'vue'
import type {
  AcpConfigOption,
  AcpConfigRejectedEvent,
  AcpPlanEntry,
  AcpProcessStatus,
  AcpToolCallEvent,
  AcpToolCallStatus,
  AcpToolCallUpdate,
  AcpUpdate,
  AcpUpdateEvent,
} from '../types/acp'
import type { MessageErrorKind, ToolCall } from '../types/session'
import type { EngineWire } from '../types/engine'
import { DEFAULT_ENGINE_WIRE } from '../types/engine'
import { acpService } from '../services/acpService'
import { onUpdate, onStatus, onConfigRejected } from '../services/acpEvents'
import { useSessionStore } from '../stores/session'
import { useWorkspaceStore } from '../stores/workspace'
import { useAgentStore } from '../stores/agent'
import { useAcpPermission } from './useAcpPermission'
import { useAcpQuestion } from './useAcpQuestion'
import { isTauri } from '../utils/isTauri'
import { tmpSessionDirSync } from '../utils/appData'
import { hasRetryMeta, parseRetryState } from '../utils/sessionUpdateMeta'

/** 本进程内已建立连接的本地会话 id 集合（connId = 本地会话 id，design §8.3 每会话一进程）。
 * 关标签 → 杀该会话进程并从集合移除；重启后为空 → 首次发送走 connect + load-or-new 重连。 */
const liveAcpSessions = new Set<string>()

/** 正在执行 session/load 的本地会话 id：load 响应前 agent 会以 session/update
 * 重放全部历史（协议 §9），本地落盘历史为准（design §4），重放事件一律忽略，
 * 否则历史消息会重复入库 */
const replayingAcpSessions = new Set<string>()

/** `${本地会话 id}:${toolCallId}` → 宿主消息（tool_call_update 无 messageId，需自持映射做累积替换）。
 * 键含 connId：每会话独立进程（design §8.1），若两个会话的 agent 偶发相同 toolCallId，
 * 也不会跨会话改写彼此的宿主消息（严格按 connId 路由，invariant 1）。 */
const toolCallHosts = new Map<string, { sessionId: string; hostId: string }>()

/** toolCallHosts 的复合键：connId（本地会话 id）+ agent 侧 toolCallId */
function toolHostKey(sessionId: string, toolCallId: string): string {
  return `${sessionId}:${toolCallId}`
}

/** 当前 turn 的会话级状态（turnId 用于消息分组与停止标记） */
const turns = new Map<string, { turnId: string }>()

/** 关闭连接进行中的会话（防重入，避免重复 acp_disconnect） */
const closingConnections = new Set<string>()

// ── 草稿输入暂存（延迟入列，PRD R5/D4） ──
// Composer 的 draft 是组件本地 state，会话切换即重置。模块级 Map 让「有未发送文字的
// 草稿」跨挂载保留（照 useConversationScroll 的 cross-mount 模式）：
// - 切走时按此判定草稿是否为空（空 → 丢弃，非空 → 保留）；
// - 切回时 Composer 由此恢复输入文字；
// - 关标签 / 丢弃草稿时清除对应项。
const draftInputs = new Map<string, string>()

/** 读取某会话的未发送输入（无记录返回空串） */
export function getDraftInput(sessionId: string): string {
  return draftInputs.get(sessionId) ?? ''
}

/** 记录某会话的未发送输入 */
export function setDraftInput(sessionId: string, text: string): void {
  draftInputs.set(sessionId, text)
}

/** 清除某会话的未发送输入（丢弃草稿时调用） */
export function clearDraftInput(sessionId: string): void {
  draftInputs.delete(sessionId)
}


// ── ACP 连接健康（每会话独立进程，design §8.3） ──
// 参考 codeg 的 composer 连接健康芯片：连接状态 ready/dead/stopped → 心形图标 + 弹层。
// 模块级 map（key = connId = 本地会话 id）：每连接独立，界面一次看一条会话。
// 由 refreshAcpStatus(connId) 主动查询 + acp://status 事件（带 connId）更新；
// 未知/从未连接时映射中无该键（前端按 null 显示「未连接」）。
type AgentInfoLike = { name: string; version: string } | null
interface AcpHealthEntry {
  status: AcpProcessStatus
  reason: string | null
  agentInfo: AgentInfoLike
}
const acpHealthMap = ref<Record<string, AcpHealthEntry>>({})

/** 读取某会话的连接健康（无记录返回 null） */
function healthOf(connId: string): AcpHealthEntry | null {
  return acpHealthMap.value[connId] ?? null
}

/** 是否有会话正在建立 ACP 连接（打开会话自动建连 / 重连进行中）。
 * 供健康芯片显示「连接中」态（跑马灯动效）。 */
const acpConnecting = ref(false)

/** 当前已建连的会话 id 列表（本地会话 id，响应式镜像）：供健康芯片按「当前会话是否已连」
 * 显示状态（每连接独立；关标签杀进程后该会话不再 live，design §8.3）。 */
const acpLiveSessionIds = ref<string[]>([])

/** 同步 liveAcpSessions（非响应式 Set）→ 响应式镜像 */
function syncLiveSessions(): void {
  acpLiveSessionIds.value = [...liveAcpSessions]
}

/** 建连/重连进行中的会话集合（模块级，与 acpConnecting 同一作用域）。
 * 必须模块级：acpConnecting 是模块级全局 ref，若此处用组件实例级 Set，
 * 多个 useConversation() 实例各自的 finally 会互相覆盖该全局标志——B 实例建连
 * 结束把 connectingSessions(空) > 0 写成 false，会误清掉 A 实例仍在建连的「连接中」态。 */
const connectingSessions = new Set<string>()
/** 重连进行中的会话数（reconnect 亦计入，避免其 finally 与建连互相覆盖标志） */
let reconnectingCount = 0
/** 重连进行中的具体会话 id（响应式镜像用；每次只允许一个 reconnect 在途） */
const reconnectingSessions = new Set<string>()

/** 建连/重连进行中的会话 id 列表（本地会话 id，响应式镜像）：健康芯片按「当前会话
 * 是否在途」显示「连接中」——每连接独立（design §8.3），不再被其它会话的建连误亮。 */
const acpConnectingSessionIds = ref<string[]>([])

/** 依据当前在途建连/重连数重算 acpConnecting（唯一写入口，保证不残留/不误清）
 * + 同步「建连中会话 id」响应式镜像（每连接独立）。 */
function recomputeAcpConnecting(): void {
  acpConnecting.value = connectingSessions.size > 0 || reconnectingCount > 0
  acpConnectingSessionIds.value = [...new Set([...connectingSessions, ...reconnectingSessions])]
}

/** 主动查询某会话连接状态并写入 acpHealthMap（best-effort：失败静默保持既有值）。
 * 每会话独立连接（design §8.3），故按 connId 查询。 */
export async function refreshAcpStatus(connId?: string): Promise<void> {
  if (!connId) return
  try {
    const s = await acpService.status(connId)
    acpHealthMap.value = {
      ...acpHealthMap.value,
      [connId]: {
        status: s.status,
        reason: s.reason ?? null,
        agentInfo: s.agentInfo ?? null,
      },
    }
  } catch {
    // 查询失败（如浏览器 mock 异常）静默忽略
  }
}

/** 读取 ACP 连接健康（供组件展示）。组件按会话说 connId 取自身健康条目。
 * acpConnectingSessionIds = 建连/重连在途的本地会话 id 列表（每连接独立）。 */
export function useAcpHealth() {
  return {
    acpHealthMap,
    healthOf,
    acpConnecting,
    acpConnectingSessionIds,
    acpLiveSessionIds,
    refreshAcpStatus,
  }
}

// ── 模型列表缓存（按引擎 wire 键；会话绑定引擎决定键位） ──
// 语义对齐 codeg 的 per-agent selectors cache：进程内缓存探针结果，
// 会话打开即可渲染真实模型列表，无需等首次发送。
const modelOptionsCache = new Map<string, AcpConfigOption[]>()
/** 并发探针守卫：连点「重新获取模型」不扇出多个子进程 */
let probeInFlight: Promise<AcpConfigOption[] | null> | null = null

/** 读取指定引擎已缓存的配置项（无则 null） */
export function getCachedModelOptions(engineId: string): AcpConfigOption[] | null {
  return modelOptionsCache.get(engineId) ?? null
}

/**
 * 真实探针刷新模型列表：调 acp_probe_options（或 mock）→ 按引擎键写缓存；
 * 传入 sessionId 时同步应用到该会话的 configOptions（下拉即时更新）。
 * 并发调用共享同一 in-flight promise。
 */
export async function refreshModelOptions(
  sessionId?: string,
): Promise<AcpConfigOption[] | null> {
  // 会话绑定的引擎决定探针目标与缓存键；无会话时回退默认引擎（含探针自身回退）
  const session = sessionId ? useSessionStore().get(sessionId) : null
  const engineId = session?.engineId ?? DEFAULT_ENGINE_WIRE
  if (!probeInFlight) {
    probeInFlight = (async () => {
      try {
        const options = await acpService.probeOptions(session?.cwd, engineId)
        if (options && options.length) {
          modelOptionsCache.set(engineId, options)
          return options
        }
        return null
      } finally {
        probeInFlight = null
      }
    })()
  }
  const options = await probeInFlight
  if (options && sessionId && session) {
    session.configOptions = options
    // 同步 session.model 到探针返回的模型 currentValue（下拉显示一致）
    const modelOpt = options.find((o) => o.id === 'model' || o.category === 'model')
    if (typeof modelOpt?.currentValue === 'string') session.model = modelOpt.currentValue
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
  const cached = getCachedModelOptions(session.engineId)
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

/** Rust 错误串 → 错误卡类别（acp.rs 约定前缀）。authText 为按引擎定制的引导文案。 */
export function classifyAcpError(err: unknown, authText?: string): { kind: MessageErrorKind; text: string } {
  const text = err instanceof Error ? err.message : String(err)
  if (text.includes('acp-dead')) {
    return { kind: 'dead', text: 'Agent 进程已退出，请重新启动后继续对话' }
  }
  if (text.includes('timeout:')) {
    return { kind: 'timeout', text: `Agent 响应超时（${text}），请稍后重试` }
  }
  if (text.includes('acp:-32000') || text.includes('Authentication required')) {
    return {
      kind: 'auth',
      text: authText ?? '当前模型需要凭据：请先完成引擎认证后重试',
    }
  }
  return { kind: 'generic', text: `生成失败：${text}` }
}

/** 按引擎 wire 给出认证引导文案（R6：不写死 opencode auth login）。 */
function authHintFor(engineId: string): string {
  switch (engineId) {
    case 'open_code':
      return '当前模型需要凭据：请在终端运行 `opencode auth login` 后重试'
    case 'claude_code':
      return '当前模型需要凭据：请在终端运行 `claude` 完成登录后重试（凭据共享 ~/.claude）'
    case 'codex':
      return '当前模型需要凭据：请在终端运行 `codex` 完成登录后重试（凭据共享 ~/.codex）'
    default:
      return '当前模型需要凭据：请先完成引擎认证后重试'
  }
}

/** 按引擎 wire 给出显示名（用于门禁提示）。 */
function engineDisplayName(engineId: string): string {
  switch (engineId) {
    case 'open_code':
      return 'OpenCode'
    case 'claude_code':
      return 'Claude Code'
    case 'codex':
      return 'Codex CLI'
    default:
      return engineId
  }
}

let updateSubscribed = false

/** 模块级订阅（一次）：acp://update → store；acp://status → 健康芯片 + 该会话错误态；
 *  acp://configRejected → 轻提示「该选项被 agent 调整」+ 乐观值回滚（AC6） */
function ensureSubscribed(): void {
  if (updateSubscribed) return
  updateSubscribed = true

  onUpdate((event: AcpUpdateEvent) => applyUpdate(event))

  onStatus((event) => {
    // 状态变更反映到该会话的健康条目（每会话独立，design §8.3）
    acpHealthMap.value = {
      ...acpHealthMap.value,
      [event.connId]: {
        status: event.status,
        reason: event.reason ?? null,
        agentInfo: acpHealthMap.value[event.connId]?.agentInfo ?? null,
      },
    }
    if (event.status === 'dead' || event.status === 'stopped') {
      liveAcpSessions.delete(event.connId)
      syncLiveSessions()
      replayingAcpSessions.delete(event.connId)
    }
    const sessionStore = useSessionStore()
    const session = sessionStore.get(event.connId)
    if (event.status === 'dead') {
      // 该会话进程死亡 → 标 error（生成中的会话由 send() 的 catch/finally 收敛，
      // 避免双份错误卡）
      if (session && session.status !== 'generating') sessionStore.setStatus(session.id, 'error')
    }
  })

  // 配置被 agent 调整为别的值（Rust 比对 set_config_option 应答产生）：
  // 乐观值回滚为实际值 + 一次性轻提示（naive-ui message 单例；组件/store 不直接 invoke）
  onConfigRejected((event: AcpConfigRejectedEvent) => {
    const sessionStore = useSessionStore()
    const session = sessionStore.get(event.connId)
    if (session) {
      const opt = session.configOptions?.find((o) => o.id === event.configId)
      if (opt) opt.currentValue = event.actual
      // model 字段同步回滚（与 setConfigOption 乐观写入对称）
      if (event.configId === 'model') session.model = event.actual
    }
    configRejectedNotice.value = { ...event, seq: configRejectedNotice.value.seq + 1 }
  })
}

/** 最近一次「配置被调整」提示（响应式）：供 UI 消费（MessageComposer 监听后 toast）。
 *  用递增 seq 让同内容的连续事件也能触发 watch。 */
export const configRejectedNotice = ref<{ seq: number } & AcpConfigRejectedEvent>({
  seq: 0,
  connId: '',
  configId: '',
  requested: '',
  actual: '',
})

function applyUpdate(event: AcpUpdateEvent): void {
  const sessionStore = useSessionStore()
  const sessionId = event.connId
  const update = event.update
  // session/load 响应前的历史重放（协议 §9）：本地落盘历史为准（design §4），全部忽略
  if (replayingAcpSessions.has(sessionId)) return
  // 事件已按 connId = 本地会话 id 路由（design §8.1），无需按 agent-side id 反查
  const session = sessionStore.get(sessionId)
  if (!session) return
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
    // plan：agent 计划清单，整体覆盖（流式增量按 entries 全量，不 append）
    case 'plan': {
      const entries: AcpPlanEntry[] = Array.isArray(update.entries) ? update.entries : []
      sessionStore.setPlan(sessionId, entries)
      break
    }
    // session_info_update：原生标题 + 可重试错误（重试横幅，非失败卡）
    case 'session_info_update': {
      if (typeof update.title === 'string' && update.title.trim()) {
        // 复用 store 的改名入口（触发索引落盘）；agent 原生标题优先于本地 applyAutoTitle
        sessionStore.renameSession(sessionId, update.title.trim())
      }
      // 重试 `_meta`：优先 update 内 `_meta`，退回事件顶层 `meta`（Rust 透传 params._meta）。
      // 仅在 `_meta` **确实携带重试信息**时才更新，避免 title/compaction 等无关
      // session_info_update 误清正在显示的重试横幅（design §3.2：仅 retry=null 或
      // prompt 响应到达才清除）。
      const meta = update._meta ?? event.meta
      if (hasRetryMeta(meta)) sessionStore.setRetry(sessionId, parseRetryState(meta))
      break
    }
    // available_commands_update：命令列表落点（供 `/` 菜单/面板，AC7）
    case 'available_commands_update': {
      sessionStore.setCommands(sessionId, update.availableCommands ?? [])
      break
    }
    // current_mode_update：当前模式（同步 mode 字段对齐既有显示，AC7）
    case 'current_mode_update': {
      sessionStore.setCurrentMode(sessionId, update.currentModeId)
      break
    }
    // 未知/未消费种类：安全忽略（AC8，不崩、不误渲染）
    default:
      break
  }
}

function applyToolCall(sessionId: string, update: AcpToolCallEvent, turnId: string | undefined): void {
  const sessionStore = useSessionStore()
  const toolCallId = update.toolCallId
  if (!toolCallId) return
  const key = toolHostKey(sessionId, toolCallId)
  const host = toolCallHosts.get(key)

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
    toolCallHosts.set(key, { sessionId, hostId: created.id })
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

/** 每个会话正在进行的建连 Promise：自动建连(打开会话)与发送(send→passGate)可能并发，
 * 共享同一建连，避免重复 spawn/session-new 产生两个进程/会话 */
const connectInFlight = new Map<string, Promise<string>>()

/** 确保 ACP 会话可用（connId = 本地会话 id）：connect(幂等) → live 直接复用；
 * 否则 load → 失败 new + 标题「（已重连）」。同会话并发调用共享同一 in-flight Promise */
function ensureAcpSession(sessionId: string): Promise<string> {
  const existing = connectInFlight.get(sessionId)
  if (existing) return existing
  const task = ensureAcpSessionInner(sessionId).finally(() => connectInFlight.delete(sessionId))
  connectInFlight.set(sessionId, task)
  return task
}

async function ensureAcpSessionInner(sessionId: string): Promise<string> {
  const sessionStore = useSessionStore()
  const session = sessionStore.get(sessionId)
  if (!session) throw new Error('acp-dead: 会话不存在')

  // 为该会话 spawn 独立进程并 initialize（幂等，design §8.1）；引擎随会话绑定
  await acpService.connect(sessionId, session.engineId)
  const cwd = session.cwd ?? tmpSessionDirSync()

  if (liveAcpSessions.has(sessionId)) {
    return session.acpSessionId ?? sessionId
  }
  if (session.acpSessionId) {
    // load 响应前 agent 会以 session/update 重放全部历史（协议 §9），
    // 本地落盘历史为准（design §4）：重放期间忽略该会话的 update，响应到达即恢复。
    // 重放事件可能晚于 load 响应到达（未验证项 probe §9#1），故除「重放期间忽略」外，
    // 再对本地历史做快照-还原：load 前后本地消息必定一致，杜绝重放内容入库。
    //
    // 快照前必须先懒加载落盘消息：自动建连（打开会话即 ensureConnected）与
    // ConversationView 的 ensureMessages 并发时，若磁盘历史在快照之后才读入，
    // 还原会覆盖掉刚加载的真实历史。await 保证快照含完整本地历史。
    await sessionStore.ensureMessages(sessionId)
    const beforeIds = new Set(session.messages.map((m) => m.id))
    // 还原本地历史：丢弃 load 重放可能追加的消息（含工具调用宿主）。
    // 但保留窗口期内用户主动发送的消息（send→ensureAcpSession 与本 load 共享
    // in-flight promise 时，用户消息会在此窗口内入库）——否则会被一并抹掉。
    const restoreLocalHistory = () => {
      session.messages = session.messages.filter((m) => beforeIds.has(m.id) || m.role === 'user')
    }
    replayingAcpSessions.add(sessionId)
    try {
      const loaded = await acpService.sessionLoad(sessionId, session.acpSessionId, cwd)
      session.acpSessionId = loaded.sessionId
      if (Array.isArray(loaded.configOptions) && loaded.configOptions.length) {
        session.configOptions = loaded.configOptions
      }
      liveAcpSessions.add(sessionId)
      syncLiveSessions()
      restoreLocalHistory()
      return loaded.sessionId
    } catch {
      // load 失败（进程重启后旧会话不可达等）→ 新建并保留本地历史（design §4）
      if (!session.title.endsWith('（已重连）')) session.title += '（已重连）'
      // 失败同样还原（重放可能已污染）
      restoreLocalHistory()
    } finally {
      replayingAcpSessions.delete(sessionId)
    }
  }
  const created = await acpService.sessionNew(sessionId, cwd)
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
          await acpService.setConfig(sessionId, created.sessionId, modelOpt.id, wanted)
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
  liveAcpSessions.add(sessionId)
  syncLiveSessions()
  return created.sessionId
}

export function useConversation() {
  const sessionStore = useSessionStore()
  const workspace = useWorkspaceStore()
  const agentStore = useAgentStore()
  const router = useRouter()
  const message = useMessage()
  const { cancelQueued } = useAcpPermission()
  const { cancelQueued: cancelQueuedQuestions } = useAcpQuestion()

  ensureSubscribed()

  /** 发送门禁（Q1）：Tauri 环境要求 installed && acpOk，否则引导跳设置页且不产生消息。
   * 检测按会话绑定的引擎执行（engineId）。 */
  async function passGate(engineId: string): Promise<boolean> {
    if (!isTauri()) return true
    if (!agentStore.detectResults[engineId]) {
      try {
        await agentStore.detect(engineId as EngineWire)
      } catch {
        // 检测失败按不可用处理
      }
    }
    const d = agentStore.detectResults[engineId]
    if (d && d.installed && d.acpOk) return true
    message.warning(`未检测到可用的 ${engineDisplayName(engineId)}，请先到设置页完成配置`)
    void router.push('/settings')
    return false
  }

  async function send(sessionId: string, content: string) {
    if (!content.trim() || workspace.isGenerating(sessionId)) return
    const engineId = sessionStore.get(sessionId)?.engineId ?? DEFAULT_ENGINE_WIRE
    if (!(await passGate(engineId))) return

    // 首次发送：草稿正式入库（延迟入列，PRD R4）。顺序必须在 ensureMessages /
    // pushMessage / applyAutoTitle 之前，保证「发送 = 入列表 + 落盘」
    if (sessionStore.isDraft(sessionId)) sessionStore.promoteDraft(sessionId)

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
      const result = await acpService.prompt(sessionId, acpSessionId, content)
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
      const { kind, text } = classifyAcpError(err, authHintFor(engineId))
      if (kind === 'dead') {
        liveAcpSessions.delete(sessionId)
        syncLiveSessions()
      }
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
      // turn 边界：可重试状态清除（成功/失败/取消任一路径，避免横幅悬挂，AC5）
      sessionStore.setRetry(sessionId, null)
      if (sessionStore.get(sessionId)?.status === 'generating') {
        sessionStore.setStatus(sessionId, 'idle')
      }
      sessionStore.touch(sessionId)
    }
  }

  /** 停止：session/cancel（44ms 内回 cancelled，probe §4）+ 本地 pending permission 回 cancelled。
   * connId = 本地会话 id（定位该会话进程）；acpSessionId = agent-side id（方法参数）。 */
  function stop(sessionId: string) {
    const session = sessionStore.get(sessionId)
    if (session?.acpSessionId) {
      cancelQueued(sessionId)
      // 挂起的 elicitation 提问同权限一样回 decline（协议 client 义务），避免 agent 阻塞
      cancelQueuedQuestions(sessionId)
      void acpService.cancel(sessionId, session.acpSessionId).catch(() => undefined)
    }
  }

  /** 关闭会话连接（标签关闭/删除会话时调用）：**杀掉该会话进程**（design §8.1/§8.3）。
   * 生成中先 cancel 止血，再 acp_disconnect（关 stdin + kill）；同步摘除 live 标记。
   * 重开时走全新 connect + load-or-new 恢复历史。best-effort（失败静默）。 */
  async function closeConnection(sessionId: string): Promise<void> {
    if (closingConnections.has(sessionId)) return
    closingConnections.add(sessionId)
    try {
      // 生成中：先 cancel 止血（stop 已含 pending permission 回 cancelled）
      if (workspace.isGenerating(sessionId)) stop(sessionId)
      // 建连可能仍在途（打开会话即自动 ensureConnected）：先等其收敛，再断连
      await connectInFlight.get(sessionId)?.catch(() => undefined)
      // 若等待期间会话已被重新打开并重新建连（connectingSessions 含它），放弃本次断连
      if (connectingSessions.has(sessionId)) return
      const session = sessionStore.get(sessionId)
      liveAcpSessions.delete(sessionId)
      syncLiveSessions()
      // 进程尚在：先优雅 session/close（best-effort），再杀进程
      if (session?.acpSessionId) {
        await acpService.closeSession(sessionId, session.acpSessionId).catch(() => undefined)
      }
      await acpService.disconnect(sessionId).catch(() => undefined)
    } finally {
      closingConnections.delete(sessionId)
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
    session: { tokenUsage: { contextWindow: number }; engineId: string },
    modelValue: string,
  ): Promise<void> {
    const slash = modelValue.indexOf('/')
    if (slash <= 0 || slash === modelValue.length - 1) return
    const provider = modelValue.slice(0, slash)
    const model = modelValue.slice(slash + 1)
    try {
      const win = await acpService.modelContextWindow(provider, model, session.engineId)
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

    if (!session.acpSessionId || !liveAcpSessions.has(sessionId)) {
      // 未建连：本地生效即可（首个 prompt 走 session/new 时再以 configOptions 为准）
      sessionStore.touch(sessionId)
      return
    }
    try {
      const opts = await acpService.setConfig(sessionId, session.acpSessionId, configId, value)
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

  /** 连接健康弹层的「重新连接」进行中标记（防重入） */
  const reconnecting = ref(false)

  /** 自动连接（打开会话时调用）：复用 ensureAcpSession 建连（每会话独立进程：
   * connect + load-or-new），best-effort 静默——失败不产错误卡（健康芯片会显示状态），
   * 不阻塞 UI。已建连/进行中的会话直接跳过（design 对齐 codeg：标签激活即 connect）。 */
  async function ensureConnected(sessionId: string): Promise<void> {
    const session = sessionStore.get(sessionId)
    if (!session) return
    if (liveAcpSessions.has(sessionId)) return
    if (connectingSessions.has(sessionId)) return
    connectingSessions.add(sessionId)
    recomputeAcpConnecting()
    try {
      await ensureAcpSession(sessionId)
    } catch {
      // 静默：失败由连接健康芯片/后续发送的错误卡呈现
    } finally {
      connectingSessions.delete(sessionId)
      recomputeAcpConnecting()
      void refreshAcpStatus(sessionId)
    }
  }

  /** 重新连接：先杀该会话进程（acp_disconnect），再全新 spawn + load-or-new（复用
   * ensureAcpSession）。不重发任何 prompt/历史；失败给 warning 提示，不产错误卡。 */
  async function reconnect(sessionId: string): Promise<void> {
    if (reconnecting.value) return
    reconnecting.value = true
    reconnectingCount += 1
    reconnectingSessions.add(sessionId)
    recomputeAcpConnecting()
    try {
      // 杀掉旧进程（可能已死/不存在，幂等），再全新建连
      await acpService.disconnect(sessionId).catch(() => undefined)
      liveAcpSessions.delete(sessionId)
      syncLiveSessions()
      await ensureAcpSession(sessionId)
      await refreshAcpStatus(sessionId)
    } catch (err) {
      message.warning(`重新连接失败：${err instanceof Error ? err.message : String(err)}`)
    } finally {
      reconnecting.value = false
      reconnectingCount = Math.max(0, reconnectingCount - 1)
      reconnectingSessions.delete(sessionId)
      recomputeAcpConnecting()
    }
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

  return { send, stop, closeConnection, regenerate, setModel, setConfigOption, fork, reconnect, reconnecting, ensureConnected }
}

/** ACP update 类型收窄辅助（组件/测试可复用） */
export type { AcpUpdate, AcpToolCallUpdate }
