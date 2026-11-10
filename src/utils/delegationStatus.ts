// 委派任务状态展示：文案 / 徽标配色 / 等待态判断。
// 父会话 transcript 的 DelegationCard 与右侧栏子任务卡片共用，避免两处各写一份映射。

import type { DelegationStatus } from '../types/delegation'

const STATUS_TEXT: Record<DelegationStatus, string> = {
  running: '运行中',
  waiting_permission: '等待授权',
  waiting_question: '等待回答',
  completed: '已完成',
  failed: '失败',
  canceled: '已取消',
  interrupted: '已中断',
}

/** 状态中文文案；未知值按运行中处理（与既有委派卡行为一致）。 */
export function delegationStatusText(status: DelegationStatus | string): string {
  return STATUS_TEXT[status as DelegationStatus] ?? '运行中'
}

/** 是否等待用户交互（权限授权 / 提问回答）。 */
export function isWaitingStatus(status: DelegationStatus | string): boolean {
  return status === 'waiting_permission' || status === 'waiting_question'
}

/** 状态徽标配色（red = 失败，amber = 等待交互，blue = 运行中，其余中性）。 */
export function delegationStatusClass(status: DelegationStatus | string): string {
  if (status === 'failed') return 'bg-red-50 text-red-600'
  if (isWaitingStatus(status)) return 'bg-amber-50 text-amber-700'
  if (status === 'running') return 'bg-blue-50 text-blue-600'
  return 'bg-neutral-100 text-neutral-500'
}
