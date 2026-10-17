// 会话数据仓库：会话与消息的增删改查、流式更新、Token 统计。
// 持久化（PRD R3 / design §4）：索引 ~/.lightr-vibe/lightr-vibe/sessions-index.json
// + 消息 sessions/<id>.json（turn 结束全量覆写，流式期间不落盘 D4）。
// 生成控制在 useConversation；本 store 只做数据与落盘。

import { defineStore } from 'pinia'
import { computed, reactive, ref } from 'vue'
import type { Message, Session, ToolCall } from '../types/session'
import type { SessionIndexEntry } from '../types/acp'
import type { EngineWire } from '../types/engine'
import { DEFAULT_ENGINE_WIRE } from '../types/engine'
import { conversationService } from '../services/conversationService'
import { fsService } from '../services/fsService'
import { createId } from '../utils/id'
import { sessionMessagesFileSync, sessionsIndexFileSync, tmpSessionDirSync } from '../utils/appData'
import { isTauri } from '../utils/isTauri'

// 示例文件夹会话（浏览器 mock 演示用；Tauri 环境启动以磁盘索引为准不注入）
function seedFolderSession(): Session {
  const now = Date.now()
  const s = conversationService.createSession('snake 游戏', 'GS/Qwen3-Coder-Next-FP8 · medium', {
    kind: 'folder',
    projectId: 'proj-29028',
    // 种子会话必须带 engineId，否则落库出现 engineId: undefined 脏记录（AC10）
    engineId: DEFAULT_ENGINE_WIRE,
  })
  s.createdAt = now - 20 * 3_600_000
  s.updatedAt = now - 17 * 3_600_000
  s.messagesLoaded = true
  s.messages = [
    {
      id: createId('msg'),
      sessionId: s.id,
      role: 'user',
      type: 'text',
      content: '写一个贪吃蛇 html',
      status: 'done',
      createdAt: s.createdAt,
    },
    {
      id: createId('msg'),
      sessionId: s.id,
      role: 'assistant',
      type: 'text',
      content: '已创建贪吃蛇游戏： snake.html\n\n使用方向键控制，撞墙或撞到自己则重新开始。',
      status: 'done',
      createdAt: s.createdAt + 60_000,
    },
  ]
  return s
}

/** 首轮用户消息截断 20 字作标题（PRD R3 标题规则） */
export function titleFromContent(content: string): string {
  const flat = content.replace(/\s+/g, ' ').trim()
  return flat.length > 20 ? `${flat.slice(0, 20)}…` : flat
}

function toIndexEntry(s: Session): SessionIndexEntry {
  return {
    id: s.id,
    title: s.title,
    kind: s.kind,
    projectId: s.projectId,
    cwd: s.cwd,
    acpSessionId: s.acpSessionId,
    model: s.model,
    mode: s.mode,
    engineId: s.engineId,
    createdAt: s.createdAt,
    updatedAt: s.updatedAt,
  }
}

function parseIndex(text: string): SessionIndexEntry[] {
  try {
    const raw: unknown = JSON.parse(text)
    if (!Array.isArray(raw)) return []
    return raw.filter(
      (e): e is SessionIndexEntry =>
        typeof e === 'object' &&
        e !== null &&
        typeof (e as SessionIndexEntry).id === 'string' &&
        typeof (e as SessionIndexEntry).title === 'string' &&
        typeof (e as SessionIndexEntry).createdAt === 'number',
    )
  } catch {
    return []
  }
}

function parseMessages(text: string, sessionId: string): Message[] {
  try {
    const raw: unknown = JSON.parse(text)
    if (!Array.isArray(raw)) return []
    return raw
      .filter(
        (m): m is Message =>
          typeof m === 'object' &&
          m !== null &&
          typeof (m as Message).id === 'string' &&
          typeof (m as Message).role === 'string' &&
          typeof (m as Message).content === 'string',
      )
      .map((m) => ({ ...m, sessionId }))
  } catch {
    return []
  }
}

function sessionFromEntry(entry: SessionIndexEntry): Session {
  const now = entry.updatedAt
  return {
    id: entry.id,
    title: entry.title,
    model: entry.model,
    status: 'idle',
    createdAt: entry.createdAt,
    updatedAt: now,
    gitBranch: entry.kind === 'folder' ? 'main' : '—',
    extensionId: createId('ext'),
    // 上下文容量权威值来自 usage_update.size（setContextUsage 覆盖）；128k 仅为
    // 收到第一次 usage_update 前的兜底占位，不代表任何具体模型的真实窗口
    tokenUsage: { input: 0, output: 0, total: 0, contextWindow: 128_000, cachedRead: 0, cachedWrite: 0 },
    messages: [],
    messagesLoaded: false,
    // 旧索引缺 engineId → 回退 open_code（AC5 向后兼容）
    engineId: entry.engineId ?? DEFAULT_ENGINE_WIRE,
    kind: entry.kind,
    projectId: entry.projectId,
    cwd: entry.cwd,
    acpSessionId: entry.acpSessionId,
    mode: entry.mode,
  }
}

/** 索引写入 debounce（design §4：1s 合并高频 touch） */
let indexSaveTimer: ReturnType<typeof setTimeout> | null = null

export const useSessionStore = defineStore('session', () => {
  const sessions = ref<Session[]>([])
  const loaded = ref(false)
  /** 懒加载在途 promise（按会话去重：并发 ensureMessages 共享同一次读盘，
   * 且发送前的 await 能等到真正完成，避免落盘覆盖未读入的历史） */
  const messageLoads = new Map<string, Promise<void>>()

  /** 草稿暂存区（延迟入列，PRD R3）：新建未发送的会话对象。
   * 不进 sessions（侧栏不显示）、不落盘（索引无此 id）；首次发送 promoteDraft 才入库。
   * 用非响应式 Map 存放（同 messageLoads 风格）；**会话对象本身 reactive 化**，
   * 使其字段（model/configOptions 等）在草稿期能触发 UI 重渲染。 */
  const drafts = new Map<string, Session>()

  const byId = computed(() => {
    const map = new Map<string, Session>()
    for (const s of sessions.value) map.set(s.id, s)
    return map
  })

  function get(id: string | null | undefined): Session | null {
    if (!id) return null
    return byId.value.get(id) ?? drafts.get(id) ?? null
  }

  // ---- 持久化 ----

  async function persistIndex(): Promise<void> {
    try {
      const entries = sessions.value.map(toIndexEntry)
      await fsService.writeText(sessionsIndexFileSync(), JSON.stringify(entries, null, 2))
    } catch {
      // 落盘失败不打断用户操作；内存态保持最新，下次变更重试
    }
  }

  function schedulePersistIndex(): void {
    if (indexSaveTimer) clearTimeout(indexSaveTimer)
    indexSaveTimer = setTimeout(() => {
      indexSaveTimer = null
      void persistIndex()
    }, 1000)
  }

  /** turn 结束落盘：整会话消息全量覆写（D4 流式期间不写） */
  async function persistMessages(sessionId: string): Promise<void> {
    const s = get(sessionId)
    if (!s) return
    try {
      await fsService.writeText(sessionMessagesFileSync(s.id), JSON.stringify(s.messages, null, 2))
    } catch {
      // 同上：失败静默，下次 turn 重试
    }
    schedulePersistIndex()
  }

  /** 启动加载：读索引（不含消息，懒加载）；浏览器 mock 注入演示种子 */
  async function load(): Promise<void> {
    try {
      const text = await fsService.readText(sessionsIndexFileSync())
      if (text !== null) {
        sessions.value = parseIndex(text).map(sessionFromEntry)
      } else if (!isTauri()) {
        sessions.value = [seedFolderSession()]
      }
    } catch {
      sessions.value = []
    }
    loaded.value = true
  }

  /** 打开标签时懒加载消息（design §4：索引恢复的会话首次打开才读盘）。
   * 读盘失败（文件损坏/IO 错误）按空历史处理并标记已加载：调用方多为
   * fire-and-forget，不允许 reject 冒泡成未处理异常；损坏文件本不可读，
   * 后续 turn 结束覆写为合法 JSON 即自愈 */
  async function ensureMessages(sessionId: string): Promise<void> {
    const s = get(sessionId)
    if (!s || s.messagesLoaded) return
    const inflight = messageLoads.get(sessionId)
    if (inflight) return inflight
    const task = (async () => {
      try {
        const text = await fsService.readText(sessionMessagesFileSync(sessionId))
        if (text !== null) s.messages = parseMessages(text, sessionId)
      } catch {
        // 见函数注释：失败按空历史处理
      } finally {
        s.messagesLoaded = true
        messageLoads.delete(sessionId)
      }
    })()
    messageLoads.set(sessionId, task)
    return task
  }

  function createSession(
    model: string,
    opts: {
      kind: Session['kind']
      projectId?: string
      cwd?: string
      /** 会话绑定的引擎 wire；缺省 open_code（弹窗选择链路由此写入） */
      engineId?: EngineWire
    },
  ): Session {
    const session = conversationService.createSession(
      `新会话 ${sessions.value.length + 1}`,
      model,
      { ...opts, engineId: opts.engineId ?? DEFAULT_ENGINE_WIRE },
    )
    session.messagesLoaded = true
    // 会话 cwd：文件夹会话绑项目 rootPath（调用方传入）；无项目 → 命名空间内 tmpsession 目录
    session.cwd = opts.cwd ?? tmpSessionDirSync()
    // 临时会话落盘路径：~/.lightr-vibe/lightr-vibe/tmpsession/<id>（命名空间硬规则）
    if (session.kind === 'tmp') session.storagePath = `${tmpSessionDirSync()}/${session.id}`
    sessions.value.unshift(session)
    schedulePersistIndex()
    return session
  }

  /**
   * 新建草稿会话（延迟入列，PRD R3/D3）：**不 unshift、不 schedulePersistIndex**。
   * 标题统一占位「新会话」（不用列表长度编号：草稿不计入列表，编号会与列表错位）；
   * 首发时 applyAutoTitle 按首条消息截断 20 字替换。
   */
  function createDraft(
    model: string,
    opts: {
      kind: Session['kind']
      projectId?: string
      cwd?: string
      engineId?: EngineWire
    },
  ): Session {
    const session = conversationService.createSession('新会话', model, {
      ...opts,
      engineId: opts.engineId ?? DEFAULT_ENGINE_WIRE,
    })
    session.messagesLoaded = true
    // 会话 cwd：文件夹会话绑项目 rootPath（调用方传入）；无项目 → tmpsession 目录
    session.cwd = opts.cwd ?? tmpSessionDirSync()
    // 临时会话落盘路径：~/.lightr-vibe/lightr-vibe/tmpsession/<id>
    if (session.kind === 'tmp') session.storagePath = `${tmpSessionDirSync()}/${session.id}`
    // reactive 化：草稿期可改字段（engineId）需驱动 composer 重渲染（Map 本身非响应式）
    const reactiveSession = reactive(session)
    drafts.set(session.id, reactiveSession)
    return reactiveSession
  }

  /** 首次发送：草稿正式入库（草稿 → sessions + 触发索引落盘，PRD R4） */
  function promoteDraft(id: string): void {
    const s = drafts.get(id)
    if (!s) return
    drafts.delete(id)
    sessions.value.unshift(s)
    schedulePersistIndex()
  }

  /** 丢弃草稿（未入库才生效，幂等）：关标签 / 空草稿切走时调用（PRD R5） */
  function discardDraft(id: string): void {
    drafts.delete(id)
  }

  /** 该会话是否仍在草稿区（未发送 = 未入库，PRD R5 定义） */
  function isDraft(id: string | null | undefined): boolean {
    return id ? drafts.has(id) : false
  }

  function renameSession(id: string, title: string) {
    const s = get(id)
    if (!s) return
    s.title = title.trim() || s.title
    s.updatedAt = Date.now()
    schedulePersistIndex()
  }

  /** 首轮用户消息确定标题（替换「新会话 N」占位，PRD R3） */
  function applyAutoTitle(id: string, firstUserContent: string) {
    const s = get(id)
    if (!s) return
    if (!s.title.startsWith('新会话')) return
    s.title = titleFromContent(firstUserContent) || s.title
    schedulePersistIndex()
  }

  /**
   * 「从此分叉」：以源会话同 projectId/cwd/kind 新建本地会话，复制从首条到
   * upToMessageId（含）的全部消息，之后独立延续。分叉会话不继承 acpSessionId
   * （不复制 ACP 连接：首次发送时按懒建路径 session/new），也不调 ACP。
   * 复制消息重分配 id 并归一化为完成态（去 streaming / 运行中工具态），
   * 保留 turnId 以维持轮次分组。落盘索引与消息，重启后可见。
   */
  function forkSession(sourceSessionId: string, upToMessageId: string): Session | null {
    const source = get(sourceSessionId)
    if (!source) return null
    const endIdx = source.messages.findIndex((m) => m.id === upToMessageId)
    // 锚点定位失败（消息已被重新生成抹掉等）：不复刻整段历史，按空历史分叉。
    // 静默复制全部会在调用方已指定截断点时产生最大意外（要求截到第 3 条却拿到全部）
    const upTo = endIdx === -1 ? [] : source.messages.slice(0, endIdx + 1)

    const forked = conversationService.createSession(`${source.title} · 分叉`, source.model, {
      kind: source.kind,
      projectId: source.projectId,
      // 分叉继承源会话引擎（会话内不切换，见 R8）
      engineId: source.engineId,
    })
    forked.cwd = source.cwd
    forked.mode = source.mode
    // configOptions 复用同一内容但不共享数组容器：分叉后 agent 侧 config_option_update
    // 是整体重赋值，浅拷贝即可隔离，避免未来就地 push 串改源会话
    forked.configOptions = source.configOptions ? [...source.configOptions] : undefined
    // 临时会话落盘路径按本会话 id 派生（与 createSession 同约定），
    // 不能沿用源会话路径，否则两个会话指向同一 per-session 目录
    if (source.kind === 'tmp') forked.storagePath = `${tmpSessionDirSync()}/${forked.id}`
    forked.messagesLoaded = true
    forked.messages = upTo.map((m) => ({
      ...m,
      // 重新分配消息 id（源会话后续变更不串扰）；工具调用 id 同样重分配
      id: createId('msg'),
      sessionId: forked.id,
      // 归一化为完成态：分叉的是已完成的对话，不携带流式/运行中残留
      status: m.status === 'error' ? 'error' : 'done',
      toolCalls: m.toolCalls?.map((tc) => ({
        ...tc,
        id: createId('tool'),
        status: tc.status === 'running' ? 'done' : tc.status,
      })),
    }))
    sessions.value.unshift(forked)
    schedulePersistIndex()
    void persistMessages(forked.id)
    return forked
  }

  function deleteSession(id: string) {
    // 防御：草稿也可能被删除流程触及（虽正常丢弃走 discardDraft）
    drafts.delete(id)
    const idx = sessions.value.findIndex((s) => s.id === id)
    if (idx !== -1) sessions.value.splice(idx, 1)
    schedulePersistIndex()
    // 消息文件 sessions/<id>.json 随索引移除即不可达；fsService 无删除能力，
    // 残留文件在命名空间内可接受（不写空文件占位）
  }

  /**
   * 移除项目时的孤儿会话收敛（子任务 1 遗留项 3）：
   * projectId 置空 + kind 转 tmp 归入「聊天」组，并触发一次索引落盘。
   */
  function orphanSessionsOf(projectId: string): void {
    let touched = false
    for (const s of sessions.value) {
      if (s.projectId === projectId) {
        s.projectId = undefined
        s.kind = 'tmp'
        touched = true
      }
    }
    if (touched) void persistIndex()
  }

  function touch(id: string) {
    const s = get(id)
    if (!s) return
    s.updatedAt = Date.now()
    schedulePersistIndex()
  }

  // ---- 消息 ----

  function pushMessage(
    sessionId: string,
    msg: Omit<Message, 'id' | 'sessionId' | 'createdAt'> & { id?: string; createdAt?: number },
  ): Message {
    const s = get(sessionId)
    const message: Message = {
      ...msg,
      id: msg.id ?? createId('msg'),
      sessionId,
      createdAt: msg.createdAt ?? Date.now(),
    }
    s?.messages.push(message)
    return message
  }

  function messageById(sessionId: string, messageId: string): Message | undefined {
    return get(sessionId)?.messages.find((m) => m.id === messageId)
  }

  function appendContent(sessionId: string, messageId: string, delta: string) {
    const m = messageById(sessionId, messageId)
    if (m) m.content += delta
  }

  function patchMessage(sessionId: string, messageId: string, patch: Partial<Message>) {
    const m = messageById(sessionId, messageId)
    if (m) Object.assign(m, patch)
  }

  function patchToolCall(sessionId: string, messageId: string, toolCallId: string, patch: Partial<ToolCall>) {
    const tc = messageById(sessionId, messageId)?.toolCalls?.find((t) => t.id === toolCallId)
    if (tc) Object.assign(tc, patch)
  }

  // ---- Token 统计 ----

  /** prompt response 的最近一次回复用量明细（inputTokens/outputTokens + 可选
   * 缓存量）：只写 input/output/cachedRead/cachedWrite，不动上下文占用 total
   * （total 由 setContextUsage 的 usage_update 拥有）。缓存量为响应绝对值，
   * 缺省按 0（不臆造），与 input/output 同为覆盖语义 */
  function addUsage(
    sessionId: string,
    usage: { input: number; output: number; cachedRead?: number; cachedWrite?: number },
  ) {
    const s = get(sessionId)
    if (!s) return
    s.tokenUsage.input = usage.input
    s.tokenUsage.output = usage.output
    s.tokenUsage.cachedRead = usage.cachedRead ?? 0
    s.tokenUsage.cachedWrite = usage.cachedWrite ?? 0
  }

  /** usage_update（probe §3：used=当前上下文占用、size=窗口容量；opencode 每 turn
   * 推送全量占用）：只更新上下文用量环的分子/分母——total=used（占用）、
   * contextWindow=size（容量，缺失时保留既有值）。不覆盖 input/output 明细
   * （那是最近一次回复的口径，见 addUsage） */
  function setContextUsage(sessionId: string, used: number, size?: number) {
    const s = get(sessionId)
    if (!s) return
    // size **只增不减**：Claude 等适配器同一会话内会把 size 从 800k 跳到 1M（甚至回落），
    // 单调取大避免分母抖动/回落（容量是会话级常量，变大必是真值，变小是适配器噪声）。
    if (size && size > 0) s.tokenUsage.contextWindow = Math.max(s.tokenUsage.contextWindow, size)
    s.tokenUsage.total = used
  }

  function setStatus(sessionId: string, status: Session['status']) {
    const s = get(sessionId)
    if (s) s.status = status
  }

  /**
   * 应用计划清单（plan update）：整体覆盖（协议增量按 entries 全量替换，不 append）。
   * 空数组同样覆盖（agent 收尾清空计划）。
   */
  function setPlan(sessionId: string, entries: Session['planEntries']) {
    const s = get(sessionId)
    if (s) s.planEntries = entries
  }

  /** 设置/清除可重试错误状态（重试横幅数据源；null = 清除）。in-memory，不落盘。 */
  function setRetry(sessionId: string, state: Session['retryState'] | null) {
    const s = get(sessionId)
    if (s) s.retryState = state ?? undefined
  }

  /** 设置该会话可用命令列表（available_commands_update，AC7 数据落点） */
  function setCommands(sessionId: string, commands: Session['availableCommands']) {
    const s = get(sessionId)
    if (s) s.availableCommands = commands
  }

  /** 设置当前模式 id（current_mode_update）；同步 mode 字段以对齐既有显示 */
  function setCurrentMode(sessionId: string, modeId: string | undefined) {
    const s = get(sessionId)
    if (!s) return
    s.currentModeId = modeId
    if (modeId) s.mode = modeId
  }

  return {
    sessions,
    loaded,
    get,
    load,
    ensureMessages,
    createSession,
    createDraft,
    promoteDraft,
    discardDraft,
    isDraft,
    renameSession,
    applyAutoTitle,
    forkSession,
    deleteSession,
    orphanSessionsOf,
    touch,
    persistIndex,
    persistMessages,
    pushMessage,
    messageById,
    appendContent,
    patchMessage,
    patchToolCall,
    addUsage,
    setContextUsage,
    setStatus,
    setPlan,
    setRetry,
    setCommands,
    setCurrentMode,
  }
})
