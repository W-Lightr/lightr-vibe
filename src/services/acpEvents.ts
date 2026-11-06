// ACP 事件注册中心：update / permission / status 三类事件的 handler 集合与分发。
// 独立模块避免 acpService ↔ acpMock 循环依赖（mock 通道直接 dispatch，
// Tauri 通道由 acpService 的 listen 回调 dispatch），消费面统一（design D5）。

import type {
  AcpConfigRejectedEvent,
  AcpPermissionEvent,
  AcpQuestionEvent,
  AcpStatusEvent,
  AcpUpdateEvent,
} from '../types/acp'
import type { DelegationRecord, DelegationStartedEvent } from '../types/delegation'

type UpdateHandler = (event: AcpUpdateEvent) => void
type PermissionHandler = (event: AcpPermissionEvent) => void
type QuestionHandler = (event: AcpQuestionEvent) => void
type StatusHandler = (event: AcpStatusEvent) => void
type ConfigRejectedHandler = (event: AcpConfigRejectedEvent) => void
type DelegationStartedHandler = (event: DelegationStartedEvent) => void
type DelegationUpdatedHandler = (event: DelegationRecord) => void

const updateHandlers = new Set<UpdateHandler>()
const permissionHandlers = new Set<PermissionHandler>()
const questionHandlers = new Set<QuestionHandler>()
const statusHandlers = new Set<StatusHandler>()
const configRejectedHandlers = new Set<ConfigRejectedHandler>()
const delegationStartedHandlers = new Set<DelegationStartedHandler>()
const delegationUpdatedHandlers = new Set<DelegationUpdatedHandler>()

export function dispatchUpdate(event: AcpUpdateEvent): void {
  for (const h of updateHandlers) h(event)
}

export function dispatchPermission(event: AcpPermissionEvent): void {
  for (const h of permissionHandlers) h(event)
}

/** 分发 elicitation 提问事件（Questions 型） */
export function dispatchQuestion(event: AcpQuestionEvent): void {
  for (const h of questionHandlers) h(event)
}

export function dispatchStatus(event: AcpStatusEvent): void {
  for (const h of statusHandlers) h(event)
}

/** 分发「配置被 agent 调整」事件（Rust 比对产生，非 wire 种类） */
export function dispatchConfigRejected(event: AcpConfigRejectedEvent): void {
  for (const h of configRejectedHandlers) h(event)
}

export function dispatchDelegationStarted(event: DelegationStartedEvent): void {
  for (const handler of delegationStartedHandlers) handler(event)
}

export function dispatchDelegationUpdated(event: DelegationRecord): void {
  for (const handler of delegationUpdatedHandlers) handler(event)
}

/** 注册 session/update 处理器；返回取消订阅函数 */
export function onUpdate(handler: UpdateHandler): () => void {
  updateHandlers.add(handler)
  return () => updateHandlers.delete(handler)
}

/** 注册 session/request_permission 处理器；返回取消订阅函数 */
export function onPermission(handler: PermissionHandler): () => void {
  permissionHandlers.add(handler)
  return () => permissionHandlers.delete(handler)
}

/** 注册 elicitation 提问处理器；返回取消订阅函数 */
export function onQuestion(handler: QuestionHandler): () => void {
  questionHandlers.add(handler)
  return () => questionHandlers.delete(handler)
}

/** 注册进程状态（dead）处理器；返回取消订阅函数 */
export function onStatus(handler: StatusHandler): () => void {
  statusHandlers.add(handler)
  return () => statusHandlers.delete(handler)
}

/** 注册 configRejected 处理器（acp_set_config 落定值被改动）；返回取消订阅函数 */
export function onConfigRejected(handler: ConfigRejectedHandler): () => void {
  configRejectedHandlers.add(handler)
  return () => configRejectedHandlers.delete(handler)
}

export function onDelegationStarted(handler: DelegationStartedHandler): () => void {
  delegationStartedHandlers.add(handler)
  return () => delegationStartedHandlers.delete(handler)
}

export function onDelegationUpdated(handler: DelegationUpdatedHandler): () => void {
  delegationUpdatedHandlers.add(handler)
  return () => delegationUpdatedHandlers.delete(handler)
}
