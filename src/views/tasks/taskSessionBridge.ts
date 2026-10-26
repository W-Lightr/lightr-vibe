// 任务 → ACP 会话的联动（子任务 3 / prd R3–R6）：启动链路 + turn 回流 + 停止 +
// 审核动作。本模块是**唯一**的编排实现：卡片、详情抽屉、重试都调这里，不各自拼流程。
//
// 为什么是普通模块而不是 composable：`onTurnSettled` 是模块级单订阅、与任何组件实例
// 无关，启动链路也只依赖 store + composable 的返回值。放在 `views/tasks/` 下符合
// directory-structure.md 的「feature-local 逻辑留在功能目录」约定（与 boardColumns.ts
// / useTaskDrag.ts 同级），不进全局 composables/。

import { useMessage } from 'naive-ui'
import { useConversation, onTurnSettled } from '../../composables/useConversation'
import type { TurnSettledPayload } from '../../composables/useConversation'
import { useProjectStore } from '../../stores/project'
import { useSessionStore } from '../../stores/session'
import { useTaskStore } from '../../stores/task'
import { useWorkspaceStore } from '../../stores/workspace'
import { isTauri } from '../../utils/isTauri'
import { taskErrorMessage } from '../../types/task'
import type { EngineWire } from '../../types/engine'
import { DEFAULT_ENGINE_WIRE } from '../../types/engine'
import type { WorkTask } from '../../types/task'

/** 一个任务的「启动令牌」：runSeq 是排队那一刻的权威值（防迟到 turn 串号），
 *  startedAt 是本 turn 实际发出提示词的时间（防 runSeq 相同但 turn 更早的回调）。 */
interface RunToken {
  runSeq: number
  startedAt: number
}

/**
 * 在途启动令牌（模块级 Map<taskId, RunToken>）：turn 回流时用它判断回调是否属于
 * 当前这次运行。Rust 侧另有 runSeq 校验兜底（task_settle_turn），这里只是把
 * 「本次启动 / 本次 turn」的时间戳带过去，避免同 runSeq 的旧 turn 影响新结论。
 */
const runTokens = new Map<string, RunToken>()

/** 正在执行启动链的任务 id（防双击；看板另有 inflight 集合做按钮置灰） */
const starting = new Set<string>()

/**
 * 回退启动失败的任务（R3「任一步失败则回退并提示」）。
 *
 * 按失败时实际所处的阶段逐级后退，保证**任何一步失败都不会留下卡在 queued /
 * preparing 的任务**：
 * - 入队失败（第 1 步）：什么都没改，无需回退（调用方直接抛原始错误）；
 * - 绑定会话 / preparing 失败：queued → todo；
 * - running 失败：preparing → todo（转表允许 preparing → failed，用 failed 更准确
 *   地表达「这次运行没起来」，并留下可读原因）。
 * 回退本身失败（stale：行已被别处改动）时保留原始错误——状态已不由本次操作掌控。
 */
async function rollbackStart(
  taskId: string,
  from: 'queued' | 'preparing',
  cause: unknown,
): Promise<never> {
  const store = useTaskStore()
  try {
    if (from === 'queued') await store.transition(taskId, 'todo', 'queued')
    else await store.transition(taskId, 'failed', 'preparing')
  } catch {
    // 回退失败不掩盖原因：下面抛出的仍是原始错误
  }
  throw new Error(taskErrorMessage(cause))
}

/** 给任务挑引擎：任务上已固化 → 否则取默认引擎（不依赖任何引擎名字分支） */
function resolveEngine(task: WorkTask): EngineWire {
  return task.engineId ?? DEFAULT_ENGINE_WIRE
}

/**
 * 启动链路（prd R3）：唯一实现，被卡片的「开始 / 重试」、详情抽屉、拖拽落列共用。
 *
 * 1. `todo|failed → queued`（failed 直接走转表里的 failed → queued）
 * 2. 拿到带 sessionId 的会话：已有 → 复用（首次发送走 session/load 续上原会话）；
 *    没有 → 新建草稿会话（尚未入列表，首次发送才入列）
 * 3. 绑定会话（attachSession，一对一）
 * 4. `queued → preparing → running`
 * 5. 打开会话标签
 * 6. 发送任务提示词
 *
 * 任一步失败：按当前阶段回退（见 rollbackStart），错误原样抛给调用方提示。
 * 项目已移除（孤儿任务）→ 直接拒绝，不创建任何东西。
 */
export async function startTask(taskId: string): Promise<void> {
  if (starting.has(taskId)) return
  const taskStore = useTaskStore()
  const sessionStore = useSessionStore()
  const workspace = useWorkspaceStore()
  const projectStore = useProjectStore()
  const conversation = useConversation()

  const task = taskStore.get(taskId)
  if (!task) throw new Error('任务不存在')
  if (task.status !== 'todo' && task.status !== 'failed') return
  if (!task.prompt.trim()) throw new Error('任务没有提示词，请先编辑补充要交给 Agent 的内容')
  const project = projectStore.get(task.projectId)
  if (!project) throw new Error('任务所属项目已被移除，无法启动（可删除或归档该任务）')

  starting.add(taskId)
  try {
    // 1 · 入队（CAS：expectedFrom = 当前状态）。这一步失败时任务没有被改动，直接抛。
    let queued: WorkTask
    try {
      queued = await taskStore.transition(taskId, 'queued', task.status)
    } catch (err) {
      throw new Error(taskErrorMessage(err))
    }
    const engineId = resolveEngine(queued)

    // 2 · 会话：已有则复用（failed 重试续用原会话），否则新建草稿。
    //     草稿不调用服务、不会失败，失败只可能发生在 3–4 步。
    let sessionId = queued.sessionId
    if (!sessionId || !sessionStore.get(sessionId)) {
      const draft = sessionStore.createDraft(workspace.selectedModel, {
        kind: 'folder',
        projectId: task.projectId,
        cwd: project.rootPath,
        engineId,
      })
      sessionId = draft.id
    }

    // 每一步失败都按**当前所处阶段**回退，绝不留卡在 queued / preparing 的行（R3）。
    // 3 · 绑定（一对一；引擎固化到任务上，重启后仍用同一引擎）
    try {
      await taskStore.attachSession(taskId, sessionId, engineId)
    } catch (err) {
      await rollbackStart(taskId, 'queued', err)
    }
    // 4 · 推进到 running（分两步，保持状态机逐段可审计）
    try {
      await taskStore.transition(taskId, 'preparing', 'queued')
    } catch (err) {
      await rollbackStart(taskId, 'queued', err)
    }
    try {
      await taskStore.transition(taskId, 'running', 'preparing')
    } catch (err) {
      await rollbackStart(taskId, 'preparing', err)
    }

    runTokens.set(taskId, { runSeq: queued.runSeq, startedAt: Date.now() })
    // 5 · 打开会话标签（用户在编码页能看到 Agent 干活）
    workspace.openSession(sessionId)
    // 5.5 · 应用任务上固化的模型（编辑器里选的）。必须在 send 之前：
    //       模型是 session/new 之后的 config option，拖到首轮之后再设就晚了。
    //       best-effort：agent 不接受该模型值时保持引擎默认，不阻断任务启动。
    if (task.modelId) {
      try {
        await conversation.setModel(sessionId, task.modelId)
      } catch {
        // 静默：模型不被支持时退回引擎默认，任务照常跑
      }
    }
    // 5.6 · 应用任务上固化的思考程度（同样在 send 之前，属于会话配置）。
    //       引擎不认这个选项时保持默认，不阻断任务。
    if (task.effortId && task.effortValue) {
      try {
        await conversation.setConfigOption(sessionId, task.effortId, task.effortValue)
      } catch {
        // 静默：该引擎不支持该选项时忽略
      }
    }
    // 6 · 发送提示词（不 await：turn 可能跑很久，结论由 onTurnSettled 回流）。
    //     `send` 内部的早退（生成中 / 未通过启用门槛）不会抛错——此时任务会停在
    //     running 且没有 turn 可回流，由详情里的「停止 + 打回」兜底（R4 的同一出口）。
    void conversation.send(sessionId, task.prompt)
  } finally {
    starting.delete(taskId)
  }
}

/** 停止（prd R4）：cancel 当前 turn **并**把任务打回待办。
 *
 *  只 cancel 而不改状态时，任务会永远停在 running/queued：按钮一直是「停止」，
 *  用户看到的就是「点了没反应」。而 queued 阶段还没有 acpSessionId，
 *  cancel 本身也会直接空转——所以状态必须由这里推进。
 *
 *  已取消（canceled）是终态且可重新启动（canceled → todo → 启动），
 *  打回待办等于给用户一个「重新来一次」的出口。 */
export async function stopTask(task: WorkTask): Promise<void> {
  // 1 · 先止血：有活动会话才 cancel（草稿 / 未建连时 acpSessionId 为空，stop 内部自会跳过）
  if (task.sessionId) useConversation().stop(task.sessionId)
  // 2 · 状态回退：queued / preparing / running / awaiting_input 都允许 → canceled
  try {
    await useTaskStore().transition(task.id, 'canceled', task.status)
  } catch (err) {
    // 状态已经被引擎推进（例如刚好结算成 review）：cancel 仍有效，不当作失败报错
    if (!taskErrorMessage(err).includes('stale')) throw err
    useMessage().info('任务状态已变化，已尝试停止当前执行')
    return
  }
  useMessage().info('已停止，可从待办重新开始')
}

/**
 * 通过 / 打回（prd R6）：状态转移交给 task store（Rust 校验）。
 * 打完回后真把会话进程杀掉（会话记录与 sessionId 保留，下次启动 session/load 续上）：
 * 留着进程只会占内存，且它的 turn 已经结算完了。
 */
export async function approveTask(task: WorkTask): Promise<void> {
  await useTaskStore().transition(task.id, 'done', 'review')
  if (task.sessionId) void useConversation().closeConnection(task.sessionId)
}

export async function rejectTask(task: WorkTask): Promise<void> {
  await useTaskStore().transition(task.id, 'todo', 'review')
  if (task.sessionId) void useConversation().closeConnection(task.sessionId)
}

// ── turn 回流（prd R5） ──────────────────────────────────────────────
// 由 `onTurnSettled` 驱动：找到该会话对应的任务 + 状态为 running + 运行令牌匹配
// → ok 到 review、空回复/其它错误到 failed 并归因；其余一律静默忽略。

let wired = false

/** 模块级单订阅（幂等）。在 `main.ts` 启动时调用一次：turn 可能在任何页面结束
 * （用户从任务页启动后切到编码页），订阅不能依赖任务页是否挂载。 */
export function wireTaskSessionBridge(): void {
  if (wired) return
  wired = true
  onTurnSettled((payload: TurnSettledPayload) => {
    void handleTurnSettled(payload)
  })
}

/** 按 sessionId 反查任务（任务与会话一对一，prd D4；一对一由 attachSession 保证） */
function taskOfSession(sessionId: string): WorkTask | null {
  return useTaskStore().tasks.find((t) => t.sessionId === sessionId) ?? null
}

async function handleTurnSettled(payload: TurnSettledPayload): Promise<void> {
  const taskStore = useTaskStore()
  const task = taskOfSession(payload.sessionId)
  // 不是任务驱动的会话（普通编码对话）→ 与改动前完全一致，什么都不做
  if (!task) return
  if (task.status !== 'running') return

  const token = runTokens.get(task.id)
  // 令牌缺失（如应用重启后会话自行续跑）→ 不能确认是本任务这次运行，忽略
  if (!token) return

  // 用户主动停止：prompt 正常返回 cancelled，`ok` 仍为 true（useConversation 里 cancelled
  // 不算错误）。「ok 但本 turn 零产出」= 被停止（真正的空回复会被判成 errorKind:'empty'），
  // 任务必须保持 running，交给用户打回/通过（R4）。
  if (payload.ok && !turnHasContent(payload.sessionId, payload.startedAt)) {
    dropRunToken(task.id, token.runSeq)
    return
  }

  // 本次 turn 早于最近一次启动 → 迟到回调，不落地（runSeq 另由后端校验兜底）。
  // 注意这个比较放在「停止」判定之后：`cancelled` 可能整 turn 零产出，两者结论相同（都丢弃）。
  if (payload.startedAt < token.startedAt) return

  // 失败：空回复 → empty_turn，其它 → agent_error。runSeq 用启动时的令牌值，
  // 用户已重试/重新启动时后端按 runSeq 不符原样返回（不写事件）。
  const outcome = payload.ok ? 'ok' : payload.errorKind === 'empty' ? 'empty' : 'error'
  const reason = payload.ok ? null : (payload.errorText ?? '生成失败')
  try {
    const row = await taskStore.settleTurn(task.id, token.runSeq, outcome, reason)
    // 后端也做了 runSeq 校验；只有真的结算了（不再是 running）才清理令牌
    if (row.status !== 'running') runTokens.delete(task.id)
  } catch (err) {
    // 回流失败不改前端猜测：提示一次，让用户手动通过/打回
    useMessage().error(`任务状态回流失败：${taskErrorMessage(err)}`)
  }
}

/**
 * 本 turn 是否产出了内容（与 `useConversation.turnHasContent` 同一定义：assistant 侧的
 * 非空文本/思考、任意工具调用、或失败卡）。只读 sessionStore，不碰任何服务。
 * 用于把「ok 但零产出」——即用户点了停止——与真正的正常结束区分开（R4）。
 */
function turnHasContent(sessionId: string, since: number): boolean {
  const session = useSessionStore().get(sessionId)
  if (!session) return false // 会话已不在索引里：拿不到证据，按「被停止」处理（保持 running）
  return session.messages.some(
    (m) =>
      m.role === 'assistant' &&
      m.createdAt >= since &&
      (m.type === 'error' || !!m.toolCalls?.length || m.content.trim() !== ''),
  )
}

/**
 * 丢弃某次运行的令牌：只在令牌仍属于该 runSeq 时删除（新一轮启动已写入新令牌则不动）。
 * 「被停止」不写库，但这次运行已经结束，令牌留着没有意义——它只用于「本 turn 属于这次运行」。
 */
function dropRunToken(taskId: string, runSeq: number): void {
  if (runTokens.get(taskId)?.runSeq === runSeq) runTokens.delete(taskId)
}

/**
 * 浏览器 dev 专用（prd R7）：手动模拟一次 turn 结论，**走与真实回流完全相同的路径**
 * （同一个 handleTurnSettled + 同一套令牌判定），只把 payload 的来源换成按钮。
 * Tauri 环境下按钮不显示（详情抽屉按 isTauri() 判定），此函数也直接拒绝。
 *
 * - `ok`    = Agent 正常结束（真实路径里有内容 → 结算为 review）
 * - `stop`  = 用户点停止（ok 但零产出 → 任务保持 running，R4）
 * - `empty` = 空回复 → failed + empty_turn
 * - `error` = 其它错误 → failed + agent_error
 */
export function simulateTurnSettle(
  task: WorkTask,
  outcome: 'ok' | 'stop' | 'empty' | 'error',
): void {
  if (isTauri()) return
  if (!task.sessionId) return
  const token = runTokens.get(task.id)
  if (!token) return
  void handleTurnSettled({
    sessionId: task.sessionId,
    ok: outcome === 'ok' || outcome === 'stop',
    // simulate 的参数名是行为名（stop/empty/error），映射到错误卡类别
    errorKind: outcome === 'empty' ? 'empty' : outcome === 'error' ? 'generic' : undefined,
    errorText:
      outcome === 'empty'
        ? 'Agent 未返回任何内容（可能已失败），请重试'
        : outcome === 'error'
          ? '模拟错误：Agent 生成失败'
          : undefined,
    startedAt: token.startedAt,
  })
}
