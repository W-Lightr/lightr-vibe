// 任务看板仓库：任务列表的唯一内存副本 + 服务层调用。
// 边界（state-management spec）：所有变更 await taskService 后用返回值更新本地行，
// 不做乐观更新——状态与时间戳的权威在 Rust（任务可能被引擎推进）；
// store 与组件永远不 invoke。所有派生分组（列 / 项目）都是 computed。

import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type {
  TaskDraft,
  TaskPatch,
  TaskStatus,
  TaskTurnOutcome,
  WorkTask,
  WorkTaskEvent,
} from '../types/task'
import type { EngineWire } from '../types/engine'
import { taskService } from '../services/taskService'

export const useTaskStore = defineStore('task', () => {
  const tasks = ref<WorkTask[]>([])
  const loaded = ref(false)

  const byId = computed(() => {
    const map = new Map<string, WorkTask>()
    for (const t of tasks.value) map.set(t.id, t)
    return map
  })

  /** 按项目分组（key 为 projectId；「全部项目」视图自行遍历 tasks） */
  const byProjectMap = computed(() => {
    const map = new Map<string, WorkTask[]>()
    for (const t of tasks.value) {
      const group = map.get(t.projectId)
      if (group) group.push(t)
      else map.set(t.projectId, [t])
    }
    return map
  })

  /** 已归档任务默认不进看板 */
  const activeTasks = computed(() => tasks.value.filter((t) => t.archivedAt === null))

  function get(id: string | null | undefined): WorkTask | null {
    return id ? (byId.value.get(id) ?? null) : null
  }

  /**
   * 项目内任务（缺省 undefined = 全部项目）。
   * projectId 为 null 表示「全部项目」视图（含项目已移除的孤儿任务，prd PAC4）。
   */
  function byProject(projectId: string | null | undefined): WorkTask[] {
    if (!projectId) return tasks.value
    return byProjectMap.value.get(projectId) ?? []
  }

  /** 用服务端返回值替换本地行（不存在则插入） */
  function upsert(row: WorkTask): void {
    const index = tasks.value.findIndex((t) => t.id === row.id)
    if (index === -1) tasks.value.push(row)
    else tasks.value[index] = row
  }

  /** 启动加载：读全部任务；失败按空列表处理（不阻塞应用启动，design §8） */
  async function load(): Promise<void> {
    try {
      tasks.value = await taskService.list()
    } catch {
      tasks.value = []
    }
    loaded.value = true
  }

  /** 新建任务：返回完整视图（含时间线）；调用方可立即打开详情 */
  async function create(draft: TaskDraft): Promise<WorkTask> {
    const details = await taskService.create(draft)
    upsert(details.task)
    return details.task
  }

  /** 编辑（仅 todo / failed；Rust 侧再次校验） */
  async function update(id: string, patch: TaskPatch): Promise<WorkTask> {
    const row = await taskService.update(id, patch)
    upsert(row)
    return row
  }

  /** 删除任务及其时间线（硬删） */
  async function remove(id: string): Promise<void> {
    await taskService.remove(id)
    tasks.value = tasks.value.filter((t) => t.id !== id)
  }

  /**
   * 状态转移：expectedFrom 传当前行状态（CAS 防并发覆盖）。
   * 失败（stale / illegal-transition）直接抛出，由调用方提示；本地不做乐观改动。
   */
  async function transition(
    id: string,
    to: TaskStatus,
    expectedFrom?: TaskStatus,
  ): Promise<WorkTask> {
    const from = expectedFrom ?? get(id)?.status ?? null
    const row = await taskService.transition(id, to, from)
    upsert(row)
    return row
  }

  /** 列内重排（项目内语义）；返回该项目的全部行（含新 sortOrder） */
  async function reorder(projectId: string, orderedIds: string[]): Promise<void> {
    const rows = await taskService.reorder(projectId, orderedIds)
    for (const row of rows) upsert(row)
  }

  /** 归档 / 取消归档（仅终态可归档） */
  async function archive(id: string, archived: boolean): Promise<WorkTask> {
    const row = await taskService.archive(id, archived)
    upsert(row)
    return row
  }

  /** 绑定会话（一对一；启动链路用） */
  async function attachSession(
    id: string,
    sessionId: string,
    engineId: EngineWire | null,
  ): Promise<WorkTask> {
    const row = await taskService.attachSession(id, sessionId, engineId)
    upsert(row)
    return row
  }

  /** 任务时间线（详情抽屉用；不缓存，时间线会随操作增长） */
  async function eventsOf(id: string): Promise<WorkTaskEvent[]> {
    return taskService.events(id)
  }

  /**
   * turn 回流（prd R5）：把 agent turn 的结论写回任务。
   * `runSeq` 由调用方在**启动时**从 `transition → queued` 的返回里取；
   * 迟到回调（runSeq 不符）后端原样返回当前行，这里不做任何假设。
   */
  async function settleTurn(
    id: string,
    runSeq: number,
    outcome: TaskTurnOutcome,
    reason: string | null,
  ): Promise<WorkTask> {
    const row = await taskService.settleTurn(id, runSeq, outcome, reason)
    upsert(row)
    return row
  }

  /** 断开会话（一对一绑定，prd D4；生成中后端拒绝） */
  async function detachSession(id: string, sessionId: string): Promise<WorkTask> {
    const row = await taskService.detachSession(id, sessionId)
    upsert(row)
    return row
  }

  /** 复制任务（同项目 / 同提示词 / 同引擎，回到待办） */
  async function duplicate(id: string): Promise<WorkTask> {
    const details = await taskService.duplicate(id)
    upsert(details.task)
    return details.task
  }

  /**
   * 写回 worktree 事实（启动链路在创建成功后调用）：路径 / 分支 / 基线 sha 只能
   * 经这条命令写入，编辑保存（update）不碰它们。
   */
  async function attachWorktree(
    id: string,
    worktreePath: string,
    workBranch: string,
    baseBranch: string,
    baseSha: string,
  ): Promise<WorkTask> {
    const row = await taskService.attachWorktree(
      id,
      worktreePath,
      workBranch,
      baseBranch,
      baseSha,
    )
    upsert(row)
    return row
  }

  /** 清空 worktree 运行态（工作树删除后调用）；失败由调用方提示，不回滚任务状态 */
  async function clearWorktree(id: string): Promise<WorkTask> {
    const row = await taskService.clearWorktree(id)
    upsert(row)
    return row
  }

  /**
   * 只写 `lastError` 的附注通道（不改状态、不改归因、不改时间戳）：
   * 启动时 init 命令失败、终态清理失败这类「不影响状态机、但用户该看到」的情况。
   *
   * 为什么不用 `update`：`task_update` 只放行 todo / failed，而这两处发生时任务
   * 已经在 running / done / canceled。失败直接抛出，由调用方决定是否吞掉。
   */
  async function finishWorktree(id: string, note: string): Promise<WorkTask> {
    const row = await taskService.worktreeIssue(id, note)
    upsert(row)
    return row
  }

  return {
    tasks,
    loaded,
    activeTasks,
    get,
    byProject,
    load,
    create,
    update,
    remove,
    transition,
    reorder,
    archive,
    attachSession,
    eventsOf,
    settleTurn,
    detachSession,
    duplicate,
    attachWorktree,
    clearWorktree,
    finishWorktree,
  }
})
