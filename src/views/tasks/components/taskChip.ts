// 状态 chip 的**单一视觉来源**（看板卡片与详情抽屉共用）。
// 状态 → 视觉分组用穷举 switch：新增 TaskStatus 未归组时编译失败，不会渲染成无色 chip。
// 失败卡片优先显示归因（空回复 / Agent 报错 / 被中断），避免所有失败都只写「失败」。

import type { WorkTask } from '../../../types/task'
import { TASK_STATUS_LABELS } from '../../../types/task'

/** chip 视觉分组（与配色一一对应） */
export type TaskChipKind = 'running' | 'attention' | 'done' | 'failed' | 'neutral'

/** chip 配色（本项目色板：蓝 = 进行中，琥珀 = 待审，红 = 失败，绿 = 完成） */
export const TASK_CHIP_CLASS: Record<TaskChipKind, string> = {
  running: 'border-blue-200 bg-blue-50 text-blue-600',
  attention: 'border-amber-200 bg-amber-50 text-amber-700',
  done: 'border-green-200 bg-green-50 text-green-700',
  failed: 'border-red-200 bg-red-50 text-red-600',
  neutral: 'border-neutral-200 bg-neutral-100 text-neutral-500',
}

/** 状态 → 视觉分组（穷举） */
export function taskChipKind(task: WorkTask): TaskChipKind {
  switch (task.status) {
    case 'queued':
    case 'preparing':
    case 'running':
    case 'merging':
      return 'running'
    case 'awaiting_input':
    case 'review':
      return 'attention'
    case 'done':
      return 'done'
    case 'failed':
      return 'failed'
    case 'todo':
    case 'canceled':
      return 'neutral'
    default: {
      const unhandled: never = task.status
      return unhandled
    }
  }
}

/** chip 文案：失败且带归因时给出更具体的原因 */
export function taskChipLabel(task: WorkTask): string {
  const base = TASK_STATUS_LABELS[task.status]
  if (task.status !== 'failed' || !task.failureReason) return base
  switch (task.failureReason) {
    case 'empty_turn':
      return `${base} · 空回复`
    case 'interrupted':
      return '已中断'
    case 'agent_error':
      return `${base} · Agent 报错`
    default: {
      const unhandled: never = task.failureReason
      return unhandled
    }
  }
}
