// 任务服务层：task_* 命令的唯一入口（tauri-bridge spec §2 边界）。
// isTauri() 双通道：Tauri 走 Rust invoke（~/.lightr-vibe/lightr-vibe/tasks.json），
// 浏览器 dev 走 taskMock 内存实现。组件 / store 永远不直接 invoke（prd AC8）。
// 参数一律 camelCase，与 Rust #[serde(rename_all = "camelCase")] 对齐（prd PAC2）。

import { invoke } from '@tauri-apps/api/core'
import type {
  TaskDetails,
  TaskDraft,
  TaskPatch,
  TaskStatus,
  TaskTurnOutcome,
  WorkTask,
  WorkTaskEvent,
} from '../types/task'
import type { EngineWire } from '../types/engine'
import { isTauri } from '../utils/isTauri'
import {
  mockArchive,
  mockAttachSession,
  mockAttachWorktree,
  mockClearWorktree,
  mockCreate,
  mockDelete,
  mockDetachSession,
  mockDuplicate,
  mockEvents,
  mockList,
  mockReorder,
  mockSettleTurn,
  mockTransition,
  mockUpdate,
  mockWorktreeIssue,
} from './taskMock'

export const taskService = {
  /** 列出任务；projectId 省略 = 全部（按 updatedAt 倒序） */
  async list(projectId?: string): Promise<WorkTask[]> {
    if (!isTauri()) return mockList(projectId)
    return invoke<WorkTask[]>('task_list', { projectId: projectId ?? null })
  },

  /** 新建任务（status = todo，追加到该项目列尾） */
  async create(draft: TaskDraft): Promise<TaskDetails> {
    if (!isTauri()) return mockCreate(draft)
    return invoke<TaskDetails>('task_create', { draft })
  },

  /** 编辑任务（仅 todo / failed 允许） */
  async update(id: string, patch: TaskPatch): Promise<WorkTask> {
    if (!isTauri()) return mockUpdate(id, patch)
    return invoke<WorkTask>('task_update', { id, patch })
  },

  /** 删除任务及其时间线 */
  async remove(id: string): Promise<void> {
    if (!isTauri()) {
      mockDelete(id)
      return
    }
    await invoke('task_delete', { id })
  },

  /**
   * 状态转移（CAS）：expectedFrom 与实际不符 → 'stale'；
   * 转换表不允许 → 'illegal-transition: <from>-><to>'（前端只做提示，权威在 Rust）。
   */
  async transition(
    id: string,
    to: TaskStatus,
    expectedFrom: TaskStatus | null,
  ): Promise<WorkTask> {
    if (!isTauri()) return mockTransition(id, to, expectedFrom)
    return invoke<WorkTask>('task_transition', { id, to, expectedFrom })
  },

  /** 列内重排：按 index 写 sortOrder（项目内语义） */
  async reorder(projectId: string, orderedIds: string[]): Promise<WorkTask[]> {
    if (!isTauri()) return mockReorder(projectId, orderedIds)
    return invoke<WorkTask[]>('task_reorder', { projectId, orderedIds })
  },

  /** 绑定 ACP 会话（一对一；启动链路用） */
  async attachSession(
    id: string,
    sessionId: string,
    engineId: EngineWire | null,
  ): Promise<WorkTask> {
    if (!isTauri()) return mockAttachSession(id, sessionId, engineId)
    return invoke<WorkTask>('task_attach_session', { id, sessionId, engineId })
  },

  /** 归档 / 取消归档（仅终态可归档） */
  async archive(id: string, archived: boolean): Promise<WorkTask> {
    if (!isTauri()) return mockArchive(id, archived)
    return invoke<WorkTask>('task_archive', { id, archived })
  },

  /** 任务时间线（升序） */
  async events(id: string): Promise<WorkTaskEvent[]> {
    if (!isTauri()) return mockEvents(id)
    return invoke<WorkTaskEvent[]>('task_events', { id })
  },

  /**
   * turn 回流（prd R5）：把一次 agent turn 的结论写回任务。
   * `runSeq` 必须是**启动时**拿到的值（`transition → queued` 的返回），
   * 与后端当前值不符即视为迟到回调，后端原样返回当前行（不报错）。
   */
  async settleTurn(
    id: string,
    runSeq: number,
    outcome: TaskTurnOutcome,
    reason: string | null,
  ): Promise<WorkTask> {
    if (!isTauri()) return mockSettleTurn(id, runSeq, outcome, reason)
    return invoke<WorkTask>('task_settle_turn', { id, runSeq, outcome, reason })
  },

  /** 断开会话（详情动作）：`sessionId` 传当前值做 CAS，生成中后端拒绝 */
  async detachSession(id: string, sessionId: string): Promise<WorkTask> {
    if (!isTauri()) return mockDetachSession(id, sessionId)
    return invoke<WorkTask>('task_detach_session', { id, sessionId })
  },

  /** 复制任务（同项目 / 同提示词 / 同引擎，回到待办） */
  async duplicate(id: string): Promise<TaskDetails> {
    if (!isTauri()) return mockDuplicate(id)
    return invoke<TaskDetails>('task_duplicate', { id })
  },

  /**
   * 写回 worktree 事实（启动链路在 `worktreeService.create` 成功后调用）。
   * path / branch / base / sha 只能经这条命令写入，`task_create` / `task_update`
   * 都不接收它们（prd R5：不接受客户端伪造）。
   */
  async attachWorktree(
    id: string,
    worktreePath: string,
    workBranch: string,
    baseBranch: string,
    baseSha: string,
  ): Promise<WorkTask> {
    if (!isTauri()) return mockAttachWorktree(id, worktreePath, workBranch, baseBranch, baseSha)
    return invoke<WorkTask>('task_attach_worktree', {
      id,
      worktreePath,
      workBranch,
      baseBranch,
      baseSha,
    })
  },

  /** 清空 worktree 运行态（工作树删除后调用）；保留 useWorktree / initCommand */
  async clearWorktree(id: string): Promise<WorkTask> {
    if (!isTauri()) return mockClearWorktree(id)
    return invoke<WorkTask>('task_clear_worktree', { id })
  },

  /**
   * 只写 `lastError`（不改状态）：init 命令失败 / 工作树清理失败的附注。
   * 与 `update` 区别：`update` 是用户编辑（仅 todo / failed），这条服务于运行中与终态。
   */
  async worktreeIssue(id: string, note: string): Promise<WorkTask> {
    if (!isTauri()) return mockWorktreeIssue(id, note)
    return invoke<WorkTask>('task_worktree_issue', { id, note })
  },
}
