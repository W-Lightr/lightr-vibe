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
import { dequeue, dropQueue, getQueueLength, onQueueFlushNeeded, requeueFront } from './useMessageQueue'
import { flushRetryDelayMs } from '../utils/queueFlush'
import { isTauri } from '../utils/isTauri'
import { tmpSessionDirSync } from '../utils/appData'
import { hasRetryMeta, parseRetryState } from '../utils/sessionUpdateMeta'
import { noticeToast, type NoticeToast } from '../utils/sessionNotice'

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

/** 队列 flush 因「门禁未通过」（引擎不可用）被拒时的退避窗口。
 *  比瞬时失败宽得多：passGate 失败每次都弹提示 + 跳设置页，短窗口会变成轰炸。 */
const FLUSH_REJECTED_WINDOW_MS = 10_000

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

// ── turn 结束回调（任务看板订阅，prd 10-17-task-review-session R5） ──
// 模块级单订阅：**未注册时是彻底的空操作**，既有会话路径逐字节不变（AC7）。
// 由 main.ts 在启动时装上（恒有订阅者）；未注册时 emitTurnSettled 直接返回。
// 回调抛错也不得影响 send() 的收尾。
export interface TurnSettledPayload {
  /** 本地会话 id（任务按 sessionId 反查） */
  sessionId: string
  /** 本 turn 是否成功（失败 = 走了错误卡分支，或被判为空回复） */
  ok: boolean
  /** 失败类别（ok = true 时为 undefined） */
  errorKind?: MessageErrorKind
  /** 可直接落盘的失败原因（错误卡正文） */
  errorText?: string
  /** 本 turn 开始的时间戳（毫秒）：任务侧用它丢弃早于本次启动的回调 */
  startedAt: number
}

let turnSettledListener: ((payload: TurnSettledPayload) => void) | null = null

/** 注册 turn 结束回调（传 null 取消）。仅供任务看板使用。 */
export function onTurnSettled(listener: ((payload: TurnSettledPayload) => void) | null): void {
  turnSettledListener = listener
}

/** 通知订阅者：未注册时直接返回（零副作用）；回调异常吞掉，绝不打断 send() */
function emitTurnSettled(payload: TurnSettledPayload): void {
  if (!turnSettledListener) return
  try {
    turnSettledListener(payload)
  } catch {
    // 订阅方是 UI 层（任务看板）：它失败不影响对话本身
  }
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

/** 本 turn 是否有「内容」（空回复判定，design §2.1）。
 *  有内容的定义：存在属于本 turnId 的任一 assistant 消息且满足其一 ——
 *  · 文本消息 content 非空；· 思考消息 content 非空；· 有工具调用（toolCalls.length）；
 *  · 已有 error 卡（refusal 等）。
 *  保守：工具或思考任一存在即算非空（纯工具回合/纯思考回合不判空，design §8 风险）。
 *  user 消息不计入（发送方本身就有一条，不作为 agent 产出内容）。 */
function turnHasContent(sessionId: string, turnId: string): boolean {
  const session = useSessionStore().get(sessionId)
  for (const m of session?.messages ?? []) {
    if (m.turnId !== turnId || m.role !== 'assistant') continue
    if (m.type === 'error') return true
    if (m.toolCalls?.length) return true
    if (m.content.trim() !== '') return true
  }
  return false
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

/** 最近一条 session notice（ACP 1.9）：供 MessageComposer 监听后弹 toast。
 *  seq 递增使重复内容也能触发；notice **绝不入 transcript**。 */
export const sessionNoticeState = ref<
  { seq: number; connId: string } & NoticeToast
>({ seq: 0, connId: '', level: 'info', title: '', description: '' })

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
        // 开始产出正文：说明前面的思考阶段已经结束，先把它收尾
        if (type === 'text') settleStreamingThinking(sessionId, turnId)
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
    // notice：ACP 1.9 Session Notices（已声明 session.notices 能力）。
    // agent 的网关警告等**只转 toast**，绝不入 transcript（不进 pushMessage/patchMessage）；
    // 解码（首行标题 / severity 映射 / 无标题丢弃）由 sessionNotice.ts 单一 owner 负责。
    case 'notice': {
      const toast = noticeToast(update)
      if (toast) {
        sessionNoticeState.value = {
          ...toast,
          seq: sessionNoticeState.value.seq + 1,
          connId: sessionId,
        }
      }
      break
    }
    // 未知/未消费种类：安全忽略（AC8，不崩、不误渲染）
    default:
      break
  }
}

/**
 * 结束本 turn 里仍在流式的思考消息。
 *
 * thinking 的「思考中…」原先只在本 turn 收尾时统一改 done，于是多轮推理
 * （思考 → 工具 → 再思考 → …）期间，前几轮的思考块会一直停在「思考中」，
 * 直到整轮结束才一起变成「思考完成」。阶段已经切换就说明那段思考结束了，
 * 这里在切换到工具调用 / 正文时立刻收尾。
 *
 * 关键：部分适配器（claude-agent-acp）整轮复用同一个 `messageId`。若只把
 * 状态改 done，后续思考会被 `status === 'done'` 挡掉，整段内容丢失。
 * 因此收尾时把 id 加一个阶段后缀「让出去」——之后同一 messageId 的 chunk
 * 会重新匹配不到、新建一条消息承载新的思考阶段，历史各段互不影响。
 */
function settleStreamingThinking(sessionId: string, turnId: string | undefined): void {
  const sessionStore = useSessionStore()
  const messages = sessionStore.get(sessionId)?.messages
  if (!messages) return
  for (const m of messages) {
    if (m.type !== 'thinking' || m.status !== 'streaming') continue
    // 只收尾本 turn 的思考块；无 turnId 的旧消息不动（由 turn 结束统一处理）
    if (turnId !== undefined && m.turnId !== turnId) continue
    const settledId = `${m.id}#settled-${Date.now().toString(36)}`
    sessionStore.patchMessage(sessionId, m.id, { id: settledId, status: 'done' })
  }
}

/** 取工具涉及的路径（ACP locations[].path），去重且过滤空值 */
function toolPaths(update: AcpToolCallUpdate): string[] | undefined {
  const paths = (update.locations ?? [])
    .map((l) => l.path)
    .filter((p): p is string => typeof p === 'string' && p.trim() !== '')
  return paths.length > 0 ? [...new Set(paths)] : undefined
}

function applyToolCall(sessionId: string, update: AcpToolCallEvent, turnId: string | undefined): void {
  const sessionStore = useSessionStore()
  const toolCallId = update.toolCallId
  if (!toolCallId) return
  const key = toolHostKey(sessionId, toolCallId)
  const host = toolCallHosts.get(key)

  if (!host) {
    // 首个 tool_call：新建宿主消息承载工具卡（同 turn 多工具各占一条）。
    // 进入工具阶段即代表上一段思考结束，先收尾它（否则整轮都在「思考中…」）。
    settleStreamingThinking(sessionId, turnId)
    const tc: ToolCall = {
      id: toolCallId,
      name: update.title ?? toolCallId,
      input: summarizeRawInput(update.rawInput),
      status: mapToolStatus(update.status),
      kind: update.kind,
      paths: toolPaths(update),
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

/** 思考强度配置项的定位规则（id 或 category 为 effort）——**单一定义**，避免两处漂移。
 *  不涉及任何引擎名判断（PAC1：差异只在 agent 返回的数据里）。 */
function findEffortOption(
  session: { configOptions?: AcpConfigOption[] },
): AcpConfigOption | undefined {
  return session.configOptions?.find((o) => o.id === 'effort' || o.category === 'effort')
}

/** 按 agent 回包写回 effort 的当前值。
 *  真实 `claude-agent-acp` 的 `session/set_config_option` 回包**只带被改的那一项**
 *  （实测：只有 effort），所以不能把回包整体当全量 configOptions 赋值（会丢 model/mode 项
 *  → 下拉变空）。这里按「以本地项为基础叠加回包项」合并；回包缺 effort 项时保留用户选择。 */
function applyEffortOptions(
  session: { configOptions?: AcpConfigOption[] },
  opts: AcpConfigOption[] | null | undefined,
  wanted: string,
): void {
  if (Array.isArray(opts) && opts.length) {
    const eff = opts.find((o) => o.id === 'effort' || o.category === 'effort')
    const local = session.configOptions ?? []
    if (eff) {
      const idx = local.findIndex((o) => o.id === eff.id)
      session.configOptions = idx >= 0
        ? local.map((o, i) => (i === idx ? { ...o, ...eff } : o))
        : [...local, eff]
      return
    }
    // 回包不含 effort：保留用户选择（乐观值），不整体替换本地配置项
    return
  }
  const eff = findEffortOption(session)
  if (eff) eff.currentValue = wanted
}

/** 回放结果：`pending` = 请求已投递但未确认（agent 超时未应答），此时**不得**回滚用户选择。 */
type EffortReplay = { ok: boolean; pending: boolean }

/** 建连（new/load）后回放用户选定的思考强度。
 *  agent 每次 `session/new` / `session/load` 都把 effort 重置为 default，
 *  若不回放，用户在未建连时（或重连后）选的「极高」会被静默丢弃 →
 *  表现为「思考强度不生效」。仅当值在 agent 暴露的选项内才发。
 *  **任何异常都不逃出**（本函数被 await 在建连路径上，抛出会断言 load/session-new 失败）。 */
async function replayPreferredEffort(
  session: { configOptions?: AcpConfigOption[]; preferredEffort?: string },
  sessionId: string,
  acpSessionId: string,
): Promise<EffortReplay> {
  try {
    const effortOpt = findEffortOption(session)
    const wanted = session.preferredEffort
    if (!effortOpt || !wanted) return { ok: true, pending: false }
    if (!collectOptionValues(effortOpt.options).includes(wanted)) return { ok: true, pending: false }
    if (effortOpt.currentValue === wanted) return { ok: true, pending: false }
    const opts = await acpService.setConfig(sessionId, acpSessionId, effortOpt.id, wanted)
    applyEffortOptions(session, opts, wanted)
    return { ok: true, pending: false }
  } catch (err) {
    // 超时：请求已投递但 agent 未应答 → 视为「已受理、未确认」，保留用户选择
    // （回滚成 default 正是本 bug 的症状）。
    return { ok: false, pending: isAcpTimeoutError(err) }
  }
}

/** 复用既有错误串契约：Rust 超时固定为 `timeout:<n>s <method>` 前缀 */
function isAcpTimeoutError(err: unknown): boolean {
  const text = err instanceof Error ? err.message : String(err)
  return text.includes('timeout:')
}

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
        // load 同样会重置 effort → 回放用户选择（失败绝不断言 load 失败）
        await replayPreferredEffort(session, sessionId, loaded.sessionId)
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
    // session/new 每次都把 effort 重置为 default → 回放用户选择（失败绝不断言建连失败）
    await replayPreferredEffort(session, sessionId, created.sessionId)
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

  /** send 的三种明确结果（design §4.1）：
   *  - `'sent'`：本轮已开始（或已正常结束）
   *  - `'would-queue'`：生成中，调用方应入队
   *  - `'rejected'`：门禁未通过（引擎不可用 / 空内容），**消息没有发出**
   *  第三态是队列回滚的关键：以前 passGate 失败返回 `undefined`，flushOnce 用
   *  `result !== 'would-queue'` 判定会把「被丢弃的消息」当成「已发出」。 */
  type SendResult = 'sent' | 'would-queue' | 'rejected'

  async function send(sessionId: string, content: string, opts?: { fromFlush?: boolean }): Promise<SendResult> {
    if (!content.trim()) return 'rejected'
    // 生成中：不在这里丢弃也不偷偷入队 —— 底层函数替调用方做决定会让调用方拿到
    // 「已发送」的假象。明确返回 'would-queue'，由输入框决定入队（design §4.1）。
    // fromFlush 是队列自己的自动发送，它已经过了门禁，必须直发。
    if (workspace.isGenerating(sessionId) && !opts?.fromFlush) return 'would-queue'
    const engineId = sessionStore.get(sessionId)?.engineId ?? DEFAULT_ENGINE_WIRE
    if (!(await passGate(engineId))) return 'rejected'

    // 首次发送：草稿正式入库（延迟入列，PRD R4）。顺序必须在 ensureMessages /
    // pushMessage / applyAutoTitle 之前，保证「发送 = 入列表 + 落盘」
    if (sessionStore.isDraft(sessionId)) sessionStore.promoteDraft(sessionId)

    // 懒加载兜底：索引恢复的会话首次发送前确保消息已读入（避免落盘覆盖丢历史）
    await sessionStore.ensureMessages(sessionId)

    const turnId = `turn-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`
    // 本 turn 的起止与结论（任务看板按 startedAt 丢弃迟到回调；未注册订阅者时
    // 这几个局部变量只被赋值、不会被读取，既有行为不变 — AC7）
    const turnStartedAt = Date.now()
    let settledOk = true
    let settledErrorKind: MessageErrorKind | undefined
    let settledErrorText: string | undefined
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
        settledOk = false
        settledErrorKind = 'generic'
        settledErrorText = 'Agent 拒绝了本次请求'
        sessionStore.pushMessage(sessionId, {
          role: 'assistant',
          type: 'error',
          content: 'Agent 拒绝了本次请求',
          status: 'error',
          errorKind: 'generic',
          turnId,
        })
      }
      // 空回复静默失败推断（D2，PRD R1）：agent 报成功（end_turn）但本 turn 零内容
      // （无文本/思考/工具/error）→ 推失败卡而非空的 done，避免「看起来成功了却什么都没有」。
      // 仅 end_turn 触发：cancelled（照旧 stopped）与 refusal（已有专门卡）均不判空。
      if (result.stopReason === 'end_turn' && !turnHasContent(sessionId, turnId)) {
        settledOk = false
        settledErrorKind = 'empty'
        settledErrorText = 'Agent 未返回任何内容（可能已失败），请重试'
        // 防御（design §2.1）：本 turn 若有残留 streaming 消息（如空内容 chunk），
        // 一并收尾为 error，避免留下空的 done 气泡与失败卡并存；正常空回复无此消息。
        for (const m of sessionStore.get(sessionId)?.messages ?? []) {
          if (m.turnId === turnId && m.status === 'streaming') {
            sessionStore.patchMessage(sessionId, m.id, { status: 'error' })
          }
        }
        sessionStore.pushMessage(sessionId, {
          role: 'assistant',
          type: 'error',
          content: 'Agent 未返回任何内容（可能已失败），请重试',
          status: 'error',
          errorKind: 'empty',
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
      // turn 结论：错误卡类别（任务看板据此把 running → failed 并归因，R5）
      settledOk = false
      settledErrorKind = kind
      settledErrorText = text
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
      // turn 边界：可重试状态清除（成功/失败/取消任一路径，避免横幅悬挂，AC5）。
      // 显式 `Object.assign` 同一 `null` 值不触发 Vue patch（setter 早退），若最后一条
      // session_info_update 未再改本会话任何字段，横幅会一直挂着；故仅在确有重试态时才写入
      // （写入 null 是一次真实状态变更）。
      if (sessionStore.get(sessionId)?.retryState !== undefined) {
        sessionStore.setRetry(sessionId, null)
      }
      if (sessionStore.get(sessionId)?.status === 'generating') {
        sessionStore.setStatus(sessionId, 'idle')
      }
      sessionStore.touch(sessionId)
      // turn 结束广播（任务看板订阅）：放在本 turn 所有收尾之后，订阅者看到的是终态；
      // 未注册订阅者时 emitTurnSettled 立即返回（AC7：既有行为完全不变）。
      // 用户主动停止（cancelled）：prompt 正常返回 cancelled，settledOk 仍为 true，
      // 订阅方按任务状态/令牌自行判断是否结算（R4：任务保持 running）。
      emitTurnSettled({
        sessionId,
        ok: settledOk,
        errorKind: settledErrorKind,
        errorText: settledErrorText,
        startedAt: turnStartedAt,
      })
      // 本轮彻底结束后再尝试消费队列（design §4.2）。放在 finally 最末尾：
      // 此时 isGenerating 已为 false、状态已收敛，flush 取队的门禁看到的是终态。
      // 但 flush 自己发出的那一轮不能再触发一次：失败时 flushOnce 会先把消息
      // requeueFront 并设好退避，这里若立刻再排一次，就会立刻把同一条取回来重发，
      // 退避形同虚设（同一条消息被连发两次）。续跑由 flushOnce 自己负责。
      if (!opts?.fromFlush) scheduleFlush(sessionId)
    }
    return 'sent'
  }

  /** 上次 flush 被拒的时间戳：退避窗口的依据（design §4.4） */
  const flushBouncedAt = new Map<string, number>()
  /** 会话已排上的 flush 定时器句柄。**它就是防重入的唯一判据**：
   *  Map 里没有句柄 = 当前没有排队的 flush。不再额外维护「已排」布尔/集合，
   *  两份状态一旦不同步（例如 clear 只清了其中一份）就会排出一个没人取消的僵尸定时器。 */
  const flushTimers = new Map<string, { timer: ReturnType<typeof setTimeout>; delayMs: number }>()

  /**
   * 安排一次队列消费。用 setTimeout(0) 而不是直接递归调用 send：
   * finally 里刚落下的状态写入需要一个干净的调用栈，否则 send 与 flush 互相嵌套。
   */
  function scheduleFlush(sessionId: string, delayMs = 0): void {
    const existing = flushTimers.get(sessionId)
    if (existing) {
      // 已经有一发在排队时，若新请求要求更早触发（delay 更小），就用更早的那个：
      // 否则一次长退避会盖住后来的「兜底 flush」，让队列多等一个窗口。
      if (existing.delayMs <= delayMs) return
      clearTimeout(existing.timer)
      flushTimers.delete(sessionId)
    }
    const timer = setTimeout(() => {
      // 先摘句柄再跑：回调里的 flushOnce / send 可能立刻再次 scheduleFlush，
      // 残留的旧句柄会让那次调用误以为「已有一发在排队」而直接返回。
      flushTimers.delete(sessionId)
      void flushOnce(sessionId)
    }, delayMs)
    flushTimers.set(sessionId, { timer, delayMs })
  }

  /**
   * 消费队首一条。
   *
   * 关键取舍是「先 dequeue 再 send，失败 requeueFront」而不是「先 peek 再 send 再删」：
   * 用队列自己的门禁去猜 send 会不会接受，一旦猜错就是「从队列取走了，然后 send
   * 静默丢弃」。先发后删让消息在任何失败路径上都回到队首，最坏是重复发一次而不是丢。
   */
  async function flushOnce(sessionId: string): Promise<void> {
    if (getQueueLength(sessionId) === 0) return
    if (workspace.isGenerating(sessionId)) return
    const wait = flushRetryDelayMs(Date.now(), flushBouncedAt.get(sessionId) ?? 0)
    if (wait > 0) {
      scheduleFlush(sessionId, wait)
      return
    }

    const item = dequeue(sessionId)
    if (!item) return

    let ok = false
    let rejected = false
    try {
      // 只有 'sent' 算成功：'would-queue'（又被拒）与 'rejected'（门禁未通过，
      // 消息根本没发出）都必须回队首，否则就是「从队列取出后静默丢弃」。
      const result = await send(sessionId, item.content, { fromFlush: true })
      ok = result === 'sent'
      rejected = result === 'rejected'
    } catch {
      ok = false
    }

    if (ok) {
      flushBouncedAt.delete(sessionId)
      // 续跑「一条接一条」。send 的 finally 对 fromFlush 这一条**故意不排**（见 send
      // 末尾注释：那里排会无视退避立刻重发同一条），所以由这里负责。
      // 关键：只在这一刻**没有**待执行的 flush 时才排（delay 0 会让 scheduleFlush
      // 用「更早者」覆盖掉等待中的退避定时器，从而绕过退避/并发打出第二次 flushOnce）。
      if (getQueueLength(sessionId) > 0 && !flushTimers.has(sessionId)) scheduleFlush(sessionId)
      return
    }
    requeueFront(sessionId, item)
    flushBouncedAt.set(sessionId, Date.now())
    // 门禁拒绝（引擎未就绪）时退避要放宽：passGate 每失败一次都会弹提示并跳设置页，
    // 1s 一次的固定窗口会变成提示轰炸。等待宽度与窗口成比例（1s → 10s）。
    const window = rejected ? FLUSH_REJECTED_WINDOW_MS : undefined
    scheduleFlush(sessionId, flushRetryDelayMs(Date.now(), flushBouncedAt.get(sessionId) ?? 0, window))
  }

  /** 丢弃该会话的队列与所有在途 flush 定时器（删会话/断连时调用） */
  function clearQueue(sessionId: string): void {
    const pending = flushTimers.get(sessionId)
    if (pending) clearTimeout(pending.timer)
    flushTimers.delete(sessionId)
    flushBouncedAt.delete(sessionId)
    dropQueue(sessionId)
  }

  // 队列侧请求消费（requestFlush）接到本实例的 scheduleFlush 上：
  // 队列逻辑不认识发送链路，反向注册避免 composable 之间循环依赖。
  onQueueFlushNeeded(scheduleFlush)

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
      // 连接已断，排队中的消息再也发不出去：连同在途 flush 定时器一并丢弃，
      // 否则定时器醒来会对一个已断开的会话反复重试退避。
      clearQueue(sessionId)
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
    session: { tokenUsage: { contextWindow: number }; engineId: string; configOptions?: AcpConfigOption[] },
    modelValue: string,
    hint?: string,
  ): Promise<void> {
    // Claude 的 model 选项 value 是单名（`fable`/`sonnet`，无 provider 前缀）——
    // 不能因缺 `/` 就早退，否则永远拿不到 Claude 的窗口（一直停在 128k 占位）。
    const slash = modelValue.indexOf('/')
    const provider = slash > 0 ? modelValue.slice(0, slash) : ''
    const model = slash > 0 ? modelValue.slice(slash + 1) : modelValue
    if (!model) return
    try {
      const win = await acpService.modelContextWindow(provider, model, session.engineId, hint)
      if (typeof win === 'number' && win > 0) session.tokenUsage.contextWindow = win
    } catch {
      // 静默：查不到就保持既有分母（usage_update.size 仍会覆盖）
    }
  }

  /** 取模型选项 name+description 作为容量提示（含 `[1M]` / `(1M context)` 等后缀）。
   *  options 可能是扁平值或分组，逐一匹配 value。 */
  function modelCapacityHint(
    session: { configOptions?: AcpConfigOption[] },
    modelValue: string,
  ): string {
    const opt = session.configOptions?.find((o) => o.id === 'model' || o.category === 'model')
    const flat: { value?: string; name?: string; description?: string }[] = []
    for (const c of opt?.options ?? []) {
      if ('value' in c) flat.push(c)
      else if ('options' in c) for (const sub of c.options ?? []) flat.push(sub)
    }
    const choice = flat.find((c) => c.value === modelValue)
    return [choice?.name, choice?.description].filter(Boolean).join(' ')
  }

  /** 通用会话配置切换（set_config_option）：model / effort 等 select 项共用。
   * 乐观更新：先写 in-memory currentValue（chip/标签即时反馈），成功后用 ACP 回包
   * 覆盖；失败（如需凭据模型 -32602）→ 回退旧值 + 错误卡而非静默（AC7）。
   * 未建 ACP 会话时仅做本地写入（首个 prompt 建连后 acp_set_config 才可用）。
   * **effort 额外记入 `preferredEffort`**：`session/new` / `session/load` 每次都把它重置为
   * default，建连后由 `replayPreferredEffort` 回放。不回放的话，用户在未建连时选的
   * 「极高」会被静默重置成 default（=「思考强度不生效」）。 */
  async function setConfigOption(
    sessionId: string,
    configId: string,
    value: string,
  ): Promise<void> {
    const session = sessionStore.get(sessionId)
    if (!session) return
    const effortOpt = findEffortOption(session)
    const isEffort = !!effortOpt && effortOpt.id === configId
    const prevValue = session.configOptions?.find((o) => o.id === configId)?.currentValue
    // 回滚快照必须在乐观写入**之前**取（失败时恢复用户原先的选择）
    const prevPreferred = session.preferredEffort
    // 乐观写入（model 额外同步 session.model 字段）
    applyOptionValue(session, configId, value)
    if (configId === 'model') session.model = value
    if (isEffort) session.preferredEffort = value

    // 切换模型即刷新上下文窗口分母（不等下一条 usage_update；best-effort 不阻塞）
    if (configId === 'model') void refreshContextWindow(session, value, modelCapacityHint(session, value))

    if (!session.acpSessionId || !liveAcpSessions.has(sessionId)) {
      // 未建连：本地生效即可。effort 已记入 preferredEffort：首个 prompt 走 session/new
      // （或对已绑定旧 acpSessionId 走 session/load）后由 replayPreferredEffort 回放；
      // 若进程仍在（仅 live 标记缺失），下一条 prompt 会复用同一 acpSessionId、
      // 不重建会话 → 这里顺手 best-effort 补发一次（失败静默，不阻断）。
      if (isEffort && session.acpSessionId) {
        void replayPreferredEffort(session, sessionId, session.acpSessionId)
      }
      sessionStore.touch(sessionId)
      return
    }
    try {
      const opts = await acpService.setConfig(sessionId, session.acpSessionId, configId, value)
      if (configId === 'model') session.model = value
      if (opts) {
        if (isEffort) {
          // effort 回包只含被改项 → 合并（不能整体替换，否则丢 model/mode 项）
          applyEffortOptions(session, opts, value)
        } else {
          session.configOptions = opts
        }
      } else {
        // 回包未含 options：保留乐观值
        applyOptionValue(session, configId, value)
      }
      sessionStore.touch(sessionId)
    } catch (err) {
      // 失败：回退到旧值（model 同步回退 session.model；effort 回退记住的选择）
      if (typeof prevValue === 'string') {
        applyOptionValue(session, configId, prevValue)
        if (configId === 'model') session.model = prevValue
      }
      if (isEffort) {
        if (prevPreferred) session.preferredEffort = prevPreferred
        else delete session.preferredEffort
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
      // 重连前若本会话正在生成：先 cancel 收敛本 turn（R4/design §5），避免旧进程旧 turn
      // 悬挂（重连会杀死该进程，在途 prompt 的响应将永不返回）。
      if (workspace.isGenerating(sessionId)) stop(sessionId)
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

  /** 主动解析并写入当前会话的上下文窗口分母（会话打开/模型项就绪时调用，best-effort）。
   * 仅在 `session.model` 有值时解析：会话未定模型时不能回退用 workspace.selectedModel
   * （那是另一个引擎的默认值，写错分母后会被单调取大永久保留）。 */
  function resolveContextWindow(sessionId: string): void {
    const session = sessionStore.get(sessionId)
    if (!session?.model) return
    void refreshContextWindow(session, session.model, modelCapacityHint(session, session.model))
  }

  /** 重新生成：移除该回合全部消息（含用户消息，send 会重新入库避免重复气泡），重发。
   *  队列非空时不重发：send 会返回 'would-queue'，而这里已经把该回合消息删了 ——
   *  结果是「消息消失了却没发出去」。队列未消化完时直接不响应，等 flush 跑完再点。 */
  function regenerate(sessionId: string, assistantMessageId: string) {
    const session = sessionStore.get(sessionId)
    const target = session?.messages.find((m) => m.id === assistantMessageId)
    if (!session || !target?.turnId || workspace.isGenerating(sessionId)) return
    if (getQueueLength(sessionId) > 0) return
    const turnId = target.turnId
    const userMsg = [...session.messages].reverse().find((m) => m.turnId === turnId && m.role === 'user')
    session.messages = session.messages.filter((m) => m.turnId !== turnId)
    if (userMsg) void send(sessionId, userMsg.content)
  }

  return { send, stop, closeConnection, regenerate, setModel, setConfigOption, fork, reconnect, reconnecting, ensureConnected, resolveContextWindow, clearQueue }
}

/** ACP update 类型收窄辅助（组件/测试可复用） */
export type { AcpUpdate, AcpToolCallUpdate }
