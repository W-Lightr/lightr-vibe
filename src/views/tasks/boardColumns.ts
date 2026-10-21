// 看板列映射（纯函数，无 Vue 依赖）：状态 → 四列的唯一真源。
// 移植 codeg board-columns.ts:22 的映射，并用穷举 switch + never 检查保证
// 新增 TaskStatus 未归列时直接编译失败，而不是从看板上静默消失（AC2）。

import type { TaskColumnId, TaskStatus, WorkTask } from '../../types/task'

export type BoardColumnId = TaskColumnId

/** 列顺序：待办 / 进行中 / 等你处理 / 已完成（AC3） */
export const BOARD_COLUMN_IDS: BoardColumnId[] = ['todo', 'inProgress', 'attention', 'done']

/** 每列收纳的状态（columnForStatus 的规格副本，两者必须一致）。
 *  queued 归「进行中」而非「待办」：它表示任务已被认领、正在等启动，
 *  放在待办会让拖拽过去后卡片先留在原列，看起来像拖拽失败。 */
export const STATUSES_BY_COLUMN: Record<BoardColumnId, TaskStatus[]> = {
  todo: ['todo'],
  inProgress: ['queued', 'preparing', 'running'],
  attention: ['awaiting_input', 'review', 'merging', 'failed'],
  done: ['done', 'canceled'],
}

/**
 * 状态 → 列。穷举 switch：漏掉任何状态都会命中 never 分支而编译失败。
 * merging 本任务不产生，但保留归入「等你处理」（兼容 codeg）。
 */
export function columnForStatus(status: TaskStatus): BoardColumnId {
  switch (status) {
    case 'todo':
      return 'todo'
    case 'queued':
    case 'preparing':
    case 'running':
      return 'inProgress'
    case 'awaiting_input':
    case 'review':
    case 'merging':
    case 'failed':
      return 'attention'
    case 'done':
    case 'canceled':
      return 'done'
    default: {
      const unhandled: never = status
      return unhandled
    }
  }
}

/** 列标题（中文，AC3，与 codeg zh-CN 一致） */
export function columnLabel(id: BoardColumnId): string {
  switch (id) {
    case 'todo':
      return '待办'
    case 'inProgress':
      return '进行中'
    case 'attention':
      // 「等你处理」比「审核」更准：该列同时装着 failed / awaiting_input / review / merging
      return '等你处理'
    case 'done':
      return '已完成'
    default: {
      const unhandled: never = id
      return unhandled
    }
  }
}

/** 空列提示文案（与 codeg EMPTY_LABEL_KEYS 对齐） */
export function emptyColumnHint(id: BoardColumnId): string {
  switch (id) {
    case 'todo':
      return '还没有待办任务'
    case 'inProgress':
      return '暂无进行中的任务'
    case 'attention':
      return '暂无等你处理的任务'
    case 'done':
      return '还没有已完成的任务'
    default: {
      const unhandled: never = id
      return unhandled
    }
  }
}

/**
 * 跨列拖拽的唯一允许集合（PRD D3）。返回目标状态，不合法返回 null：
 * 前端据此决定「列是否高亮」，真正校验仍在 Rust（PAC3）。
 */
export function allowedDrop(from: TaskStatus, column: BoardColumnId): TaskStatus | null {
  if (from === 'todo' && column === 'inProgress') return 'queued'
  if (from === 'review' && column === 'done') return 'done'
  if (from === 'review' && column === 'todo') return 'todo'
  return null
}

/**
 * 按列分组：先过滤已取消（showCanceled=false）与已归档（showArchived=false），
 * 再按 updatedAt 倒序（最新动作置顶，与 codeg byFreshest 一致）。
 * 返回值为新数组，不改动入参。
 */
export function groupTasksByColumn(
  tasks: WorkTask[],
  opts: { showCanceled: boolean; showArchived: boolean },
): Record<BoardColumnId, WorkTask[]> {
  const grouped: Record<BoardColumnId, WorkTask[]> = {
    todo: [],
    inProgress: [],
    attention: [],
    done: [],
  }
  for (const task of tasks) {
    if (!opts.showCanceled && task.status === 'canceled') continue
    if (!opts.showArchived && task.archivedAt !== null) continue
    grouped[columnForStatus(task.status)].push(task)
  }
  for (const id of BOARD_COLUMN_IDS) {
    grouped[id].sort((a, b) => b.updatedAt - a.updatedAt)
  }
  return grouped
}
