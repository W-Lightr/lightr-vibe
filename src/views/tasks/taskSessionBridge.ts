// 任务 → ACP 会话的联动（子任务 3 / prd R3–R6）：启动链路 + turn 回流 + 停止 +
// 审核动作 + git worktree 生命周期（子任务 10-26-worktree-lifecycle / R1–R4）。
// 本模块是**唯一**的编排实现：卡片、详情抽屉、重试都调这里，不各自拼流程。
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
import { worktreeService } from '../../services/worktreeService'
import { isTauri } from '../../utils/isTauri'
import { taskErrorMessage } from '../../types/task'
import type { EngineWire } from '../../types/engine'
import { DEFAULT_ENGINE_WIRE } from '../../types/engine'
import type { TaskStatus, WorkTask } from '../../types/task'
import { SETUP_ERROR_PREFIX, WORKTREE_CLEANUP_FAILED_PREFIX } from './components/setupErrorPrefix'

export { SETUP_ERROR_PREFIX }

/** 一个任务的「启动令牌」：runSeq 是排队那一刻的权威值（防迟到 turn 串号），
 *  startedAt 是本 turn 实际发出提示词的时间（防 runSeq 相同但 turn 更早的回调）。 */
interface RunToken {
  runSeq: number
  startedAt: number
}

/** 删除工作树所需的事实：分支 / 基线可缺省 → 回落到任务行上的值。
 *  `WorkTask` 与 `CreatedWorktree` 都直接满足它；`worktreePath` 为 null 时删不了，
 * 由 `discardWorktree` 判空后返回 null（没有「删一个空路径」这回事）。 */
interface WorktreeFacts {
  worktreePath: string | null
  workBranch?: string | null
  baseBranch?: string | null
  baseSha?: string | null
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
 *
 * worktree 版本（子任务 lifecycle R2）：`created` 非空表示本次启动已经建出工作树，
 * 回退前**先**把它删掉，不留孤儿目录；删除本身失败只记录（`lastError` 里带上路径），
 * 不掩盖原始错误——目录残留是可见的，卡在 queued 才是不可恢复的。
 */
async function rollbackStart(
  taskId: string,
  from: 'queued' | 'preparing',
  cause: unknown,
  created: WorktreeFacts | null = null,
): Promise<never> {
  const store = useTaskStore()
  // 先删目录、再回退状态：顺序反了的话行上已没有路径，删除入口就丢了（孤儿目录）。
  // 删除失败只留附注（带上路径），回退照常进行——卡在 queued 才是不可恢复的。
  if (created) {
    const failure = await discardWorktree(taskId, created)
    if (failure) {
      await noteWorktreeIssue(
        taskId,
        `${WORKTREE_CLEANUP_FAILED_PREFIX}${created.worktreePath}（${failure}），请在详情里重试清理`,
      )
    }
  }
  try {
    if (from === 'queued') await store.transition(taskId, 'todo', 'queued')
    else await store.transition(taskId, 'failed', 'preparing')
  } catch {
    // 回退失败不掩盖原因：下面抛出的仍是原始错误
  }
  throw new Error(taskErrorMessage(cause))
}

/**
 * 删掉任务当前登记的工作树并清空运行态字段；失败只返回错误串（不抛），由调用方决定
 * 怎么呈现。入参 `known` 是**已知**的工作树事实：链路里刚拿到的 `create` 返回值、
 * 或转移返回行上的字段，优先使用——`taskStore.get()` 是另一份快照，可能还没跟上
 * （例如 `attachWorktree` 写库成功但 store 更新失败），那时按 store 的 `workBranch ?? ''`
 * 调用 git 等于拿空分支名去删。
 *
 * 删除失败时**保留** `worktreePath`：详情抽屉据此显示「清理失败 + 路径 + 重试」，
 * 用户重试走的也是这条路径（第二次 `worktree remove` 对已删目录视为成功）。
 * 任务已被删除 → 没什么可清的；项目已被移除（孤儿任务）→ 只清登记，目录留给用户手工处理
 * （没有 rootPath 就无从调用 git，硬报错会让重试按钮永远转不动）。
 */
async function discardWorktree(
  taskId: string,
  known: WorktreeFacts,
): Promise<string | null> {
  const path = known.worktreePath
  if (!path) return null
  const taskStore = useTaskStore()
  const task = taskStore.get(taskId)
  if (!task) return null
  const project = useProjectStore().get(task.projectId)
  if (project) {
    try {
      await worktreeService.remove(
        project.rootPath,
        path,
        known.workBranch ?? task.workBranch ?? '',
        known.baseBranch ?? task.baseBranch,
        known.baseSha ?? task.baseSha,
      )
    } catch (err) {
      return String(err instanceof Error ? err.message : err) || '未知错误'
    }
  }
  try {
    await taskStore.clearWorktree(taskId)
  } catch (err) {
    return `任务记录未能更新：${taskErrorMessage(err)}`
  }
  return null
}

/** 工作树相关错误串 → 用户可读中文（Rust 侧约定的错误前缀，service 原样透传） */
function worktreeErrorMessage(err: unknown): string {
  const msg = err instanceof Error ? err.message : String(err ?? '')
  if (msg.startsWith('worktree-not-repo')) return `当前项目不是 git 仓库：${afterColon(msg)}`
  if (msg.startsWith('worktree-path-exists')) return `工作树目录已存在，请先手动清理：${afterColon(msg)}`
  if (msg.startsWith('worktree-base-branch')) return `无法使用所选基线分支：${afterColon(msg)}`
  if (msg.startsWith('worktree-base-sha')) return `读取基线提交失败：${afterColon(msg)}`
  if (msg === 'worktree-detached') return '工作树没有挂在任务分支上（HEAD 游离），已回滚'
  if (msg.startsWith('worktree-add')) return `创建 git worktree 失败：${afterColon(msg)}`
  if (msg.startsWith('worktree-remove')) return `删除 git worktree 失败：${afterColon(msg)}`
  if (msg === 'path-not-absolute') return '工作树路径不是绝对路径，已拒绝'
  if (msg === 'path-not-utf8') return '工作树路径含无法识别的字符，已拒绝'
  return msg || '创建 git worktree 失败'
}

/** 取错误串里第一个冒号之后的部分（`worktree-add: <stderr>` → `<stderr>`） */
function afterColon(msg: string): string {
  const index = msg.indexOf(':')
  return index === -1 ? msg : msg.slice(index + 1).trim() || msg
}

/**
 * 把一条说明写进任务的 `lastError`，**不改变状态**（R2 init 失败 / R3 清理失败共用）。
 *
 * 走 `finishWorktree` 这条专用通道而不是 `task_update`：后者只放行 todo / failed，
 * 而这里的目标状态是 running（init 失败）或 done / canceled（清理失败）。
 * 失败静默——它只是附注，不能反过来打断任务。
 */
async function noteWorktreeIssue(taskId: string, note: string): Promise<void> {
  try {
    await useTaskStore().finishWorktree(taskId, note)
  } catch {
    // 附注写不进去不影响主流程：错误已经通过 toast / 详情文案表达了
  }
}

/** 给任务挑引擎：任务上已固化 → 否则取默认引擎（不依赖任何引擎名字分支） */
function resolveEngine(task: WorkTask): EngineWire {
  return task.engineId ?? DEFAULT_ENGINE_WIRE
}

/**
 * 启动链路（prd R3）：唯一实现，被卡片的「开始 / 重试」、详情抽屉、拖拽落列共用。
 *
 * 1. `todo|failed → queued`（failed 直接走转表里的 failed → queued）
 * 2. 决定工作目录：勾了 worktree 就先建工作树（R1），否则就是项目根目录
 * 3. 拿到带 sessionId 的会话：已有 → 复用（首次发送走 session/load 续上原会话）；
 *    没有 → 新建草稿会话（尚未入列表，首次发送才入列）
 * 4. 绑定会话（attachSession，一对一）
 * 5. `queued → preparing → running`
 * 6. 打开会话标签
 * 7. 发送任务提示词
 *
 * 任一步失败：按当前阶段回退（见 rollbackStart），错误原样抛给调用方提示；
 * 已建出的工作树在回退前先删（R2，不留孤儿目录）。项目已移除（孤儿任务）→ 直接拒绝。
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
  /** 本次启动建出的工作树（回退时需要删掉的路径）；null = 本次没建 */
  let createdWorktree: WorktreeFacts | null = null
  try {
    // 1 · 入队（CAS：expectedFrom = 当前状态）。这一步失败时任务没有被改动，直接抛。
    let queued: WorkTask
    try {
      queued = await taskStore.transition(taskId, 'queued', task.status)
    } catch (err) {
      throw new Error(taskErrorMessage(err))
    }
    const engineId = resolveEngine(queued)

    // 2 · 工作目录（R1）：勾了 worktree 就在独立工作树里跑，否则与改动前完全一致。
    //     worktree 创建失败 → failed(setup_error)，**不建会话**（AC7）。
    //     关掉开关的任务（useWorktree=false）在这里**一次 git 都不碰**（AC1）。
    let cwd = project.rootPath
    if (queued.useWorktree && queued.worktreePath) {
      // 勾着开关但行上还挂着路径（上一轮清理失败 / 应用中途退出）：先清掉再重建。
      // 关掉开关时不动它——那是用户的事，不在这里偷偷删目录（AC1）。
      // 清不掉就别重建——重建必然撞「路径已存在」；把清理失败报到详情里。
      const stale = await discardWorktree(taskId, queued)
      if (stale) {
        await failSetup(taskId, `上一轮的工作树未能清理（${stale}），请先在详情里重试清理`)
      }
    }
    if (queued.useWorktree) {
      try {
        const info = await worktreeService.create({
          rootPath: project.rootPath,
          // null / 空 = 由 Rust 解析为仓库当前分支
          baseBranch: queued.baseBranch,
          taskId: queued.id,
          initCommand: queued.initCommand,
        })
        // 先记下本次建出的工作树：后面任何一步失败都要靠它回滚（AC8）
        createdWorktree = info
        await taskStore.attachWorktree(
          taskId,
          info.worktreePath,
          info.workBranch,
          info.baseBranch,
          info.baseSha,
        )
        cwd = info.worktreePath
        // init 命令失败不阻断启动（R2 / AC10）：只留一条附注供用户参考
        if (info.initError) await noteWorktreeIssue(taskId, `初始化命令失败：${info.initError}`)
      } catch (err) {
        await failSetup(taskId, worktreeErrorMessage(err), createdWorktree)
      }
    }

    // 3 · 会话：已有则复用（failed 重试续用原会话），否则新建草稿。
    //     草稿不调用服务、不会失败，失败只可能发生在 4–5 步。
    let sessionId = queued.sessionId
    if (!sessionId || !sessionStore.get(sessionId)) {
      const draft = sessionStore.createDraft(workspace.selectedModel, {
        kind: 'folder',
        projectId: task.projectId,
        cwd,
        engineId,
      })
      sessionId = draft.id
    }

    // 每一步失败都按**当前所处阶段**回退，绝不留卡在 queued / preparing 的行（R3）。
    // 4 · 绑定（一对一；引擎固化到任务上，重启后仍用同一引擎）
    try {
      await taskStore.attachSession(taskId, sessionId, engineId)
    } catch (err) {
      await rollbackStart(taskId, 'queued', err, createdWorktree)
    }
    // 5 · 推进到 running（分两步，保持状态机逐段可审计）
    try {
      await taskStore.transition(taskId, 'preparing', 'queued')
    } catch (err) {
      await rollbackStart(taskId, 'queued', err, createdWorktree)
    }
    try {
      await taskStore.transition(taskId, 'running', 'preparing')
    } catch (err) {
      await rollbackStart(taskId, 'preparing', err, createdWorktree)
    }

    runTokens.set(taskId, { runSeq: queued.runSeq, startedAt: Date.now() })
    // 6 · 打开会话标签（用户在编码页能看到 Agent 干活）
    workspace.openSession(sessionId)
    // 6.5 · 应用任务上固化的模型（编辑器里选的）。必须在 send 之前：
    //       模型是 session/new 之后的 config option，拖到首轮之后再设就晚了。
    //       best-effort：agent 不接受该模型值时保持引擎默认，不阻断任务启动。
    if (task.modelId) {
      try {
        await conversation.setModel(sessionId, task.modelId)
      } catch {
        // 静默：模型不被支持时退回引擎默认，任务照常跑
      }
    }
    // 6.6 · 应用任务上固化的思考程度（同样在 send 之前，属于会话配置）。
    //       引擎不认这个选项时保持默认，不阻断任务。
    if (task.effortId && task.effortValue) {
      try {
        await conversation.setConfigOption(sessionId, task.effortId, task.effortValue)
      } catch {
        // 静默：该引擎不支持该选项时忽略
      }
    }
    // 7 · 发送提示词（不 await：turn 可能跑很久，结论由 onTurnSettled 回流）。
    //     `send` 内部的早退（生成中 / 未通过启用门槛）不会抛错——此时任务会停在
    //     running 且没有 turn 可回流，由详情里的「停止 + 打回」兜底（R4 的同一出口）。
    void conversation.send(sessionId, task.prompt)
  } finally {
    starting.delete(taskId)
  }
}

/**
 * 启动失败于「建工作树」之前/之中（R2 / AC7）：任务 `failed(setup_error)`，
 * `lastError` 留可读原因，**不建会话**，抛出让调用方 toast。
 *
 * 「setup_error」不是独立的 `failureReason` 枚举值（Rust `TaskFailureReason` 只有
 * agent_error / interrupted / empty_turn，前端 `types/task.ts` 同）：它由状态 `failed`
 * + 带「准备失败」前缀的 `lastError` 表达，chip 显示「失败 · 启动失败」（taskChip.ts）。
 *
 * 为什么不是 `queued → todo`：工作目录都没准备好，这次启动是「没起来」，用 failed
 * 表达比回到待办更准确（待办看起来像用户自己取消的），也保留失败卡片上的原因。
 *
 * 为什么是 `queued → preparing → failed`：转表里 queued **只能**到 preparing / todo /
 * canceled（task.rs TRANSITIONS），没有 queued → failed，多跳一步是唯一合法路径。
 *
 * 必须先回滚工作树（`created` 非空 = `-b` 建出了分支但写库失败那一步）：否则任务
 * 看起来「没起来」，磁盘上却留着一个工作树和分支，下次启动必然撞名。
 * 所以这里**同时**保证「failed」与「无孤儿目录」——只成功一半都不可接受。
 */
async function failSetup(
  taskId: string,
  reason: string,
  created: WorktreeFacts | null = null,
): Promise<never> {
  const store = useTaskStore()
  if (created) {
    const failure = await discardWorktree(taskId, created)
    if (failure) reason = `${reason}（工作树未能清理：${created.worktreePath}，${failure}）`
  }
  let failed = false
  try {
    await store.transition(taskId, 'preparing', 'queued')
    await store.finishWorktree(taskId, `${SETUP_ERROR_PREFIX}${reason}`)
    await store.transition(taskId, 'failed', 'preparing')
    failed = true
  } catch {
    // 状态推进失败不掩盖原因：下面抛出的仍是原始原因
  }
  // 兜底：没能落到 failed 时至少别把行卡在 queued（那会永久占住「启动」按钮），
  // 退回 todo —— 用户可以再点一次。这里刻意不复用 rollbackStart（它会重新抛）
  if (!failed) {
    try {
      await store.transition(taskId, 'todo', 'queued')
      await store.finishWorktree(taskId, `${SETUP_ERROR_PREFIX}${reason}`)
    } catch {
      // 行已被别处改走：本次操作不再拥有它
    }
  }
  throw new Error(reason)
}

/**
 * 终态清理（design §3.5 / R3）：任务刚进入 `done` / `canceled` 且有工作树 → 删掉它。
 *
 * 三个调用点（通过 / 停止 / 看板拖到取消列）都走这里，避免各写一份、行为漂移。
 * 删除失败**不回滚状态**（AC9）：任务已经跑完了，状态才是用户看到的事实；
 * 失败原因写进 `lastError`（含路径），详情抽屉据此显示「工作树清理失败」+ 重试按钮。
 *
 * `row` 用转移的返回值（含路径 / 分支 / sha），不重新查 store——转移刚写完，这里就是最新。
 */
async function cleanupWorktreeOnFinish(row: WorkTask): Promise<void> {
  const finished = row.status === 'done' || row.status === 'canceled'
  if (!finished || !row.worktreePath) return
  const failure = await discardWorktree(row.id, row)
  if (!failure) return
  await noteWorktreeIssue(row.id, `${WORKTREE_CLEANUP_FAILED_PREFIX}${row.worktreePath}（${failure}）`)
  useMessage().warning(`工作树清理失败：${row.worktreePath}，可在任务详情里重试`)
}

/**
 * 看板拖拽落列用的状态转移（唯一入口的薄包装）：保留 `taskStore.transition` 的 CAS
 * 与错误语义，只在落到 `done` / `canceled` 时补一步工作树清理（R3）。
 *
 * 为什么不直接调 store：终态清理由本模块独占，三个落点（通过 / 停止 / 拖拽）必须
 * 走同一条路径，否则总有一个漏删。其它状态原样透传，行为与改动前一致。
 */
export async function transitionTask(
  taskId: string,
  to: TaskStatus,
  from: TaskStatus,
): Promise<WorkTask> {
  const row = await useTaskStore().transition(taskId, to, from)
  await cleanupWorktreeOnFinish(row)
  return row
}

/**
 * 详情抽屉「重试清理」入口（R3 / AC9）：删掉任务当前登记的工作树并清空运行态。
 * 成功 / 失败都会重新渲染；失败原因写回 `lastError` 并抛出，由调用方 toast。
 *
 * 失败**不回滚状态**：任务早已是终态，重试只是收拾残局（AC9 的同一约定）。
 */
export async function retryWorktreeCleanup(taskId: string): Promise<void> {
  const task = useTaskStore().get(taskId)
  if (!task?.worktreePath) return
  const failure = await discardWorktree(taskId, task)
  if (!failure) {
    useMessage().success('工作树已清理')
    return
  }
  await noteWorktreeIssue(taskId, `${WORKTREE_CLEANUP_FAILED_PREFIX}${task.worktreePath}（${failure}）`)
  throw new Error(failure)
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
  let canceled: WorkTask
  try {
    canceled = await useTaskStore().transition(task.id, 'canceled', task.status)
  } catch (err) {
    // 状态已经被引擎推进（例如刚好结算成 review）：cancel 仍有效，不当作失败报错
    if (!taskErrorMessage(err).includes('stale')) throw err
    useMessage().info('任务状态已变化，已尝试停止当前执行')
    return
  }
  // 3 · 进终态即删工作树（R3 / AC6）：失败只提示，不回滚上面的取消
  await cleanupWorktreeOnFinish(canceled)
  useMessage().info('已停止，可从待办重新开始')
}

/**
 * 通过 / 打回（prd R6）：状态转移交给 task store（Rust 校验）。
 * 打完回后真把会话进程杀掉（会话记录与 sessionId 保留，下次启动 session/load 续上）：
 * 留着进程只会占内存，且它的 turn 已经结算完了。
 * 通过 = 进终态，工作树随之删除（R3 / AC5）。
 */
export async function approveTask(task: WorkTask): Promise<void> {
  const done = await useTaskStore().transition(task.id, 'done', 'review')
  if (task.sessionId) void useConversation().closeConnection(task.sessionId)
  await cleanupWorktreeOnFinish(done)
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
