// 子代理委派域模型：独立于任务看板 Task，严格对齐 Rust camelCase wire 契约。
import type { EngineWire } from './engine'

export type DelegationStatus =
  | 'running'
  | 'waiting_permission'
  | 'waiting_question'
  | 'completed'
  | 'failed'
  | 'canceled'
  | 'interrupted'

export interface DelegationRecord {
  taskId: string
  parentSessionId: string
  childSessionId: string
  agentType: EngineWire
  task: string
  cwd: string
  status: DelegationStatus
  createdAt: number
  updatedAt: number
  errorCode?: string
  message?: string
  summary?: string
}

export interface DelegationSettings {
  enabled: boolean
  depthLimit: number
}

export interface DelegationStartedEvent { record: DelegationRecord }
