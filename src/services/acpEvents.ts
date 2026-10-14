// ACP 事件注册中心：update / permission / status 三类事件的 handler 集合与分发。
// 独立模块避免 acpService ↔ acpMock 循环依赖（mock 通道直接 dispatch，
// Tauri 通道由 acpService 的 listen 回调 dispatch），消费面统一（design D5）。

import type { AcpPermissionEvent, AcpQuestionEvent, AcpStatusEvent, AcpUpdateEvent } from '../types/acp'

type UpdateHandler = (event: AcpUpdateEvent) => void
type PermissionHandler = (event: AcpPermissionEvent) => void
type QuestionHandler = (event: AcpQuestionEvent) => void
type StatusHandler = (event: AcpStatusEvent) => void

const updateHandlers = new Set<UpdateHandler>()
const permissionHandlers = new Set<PermissionHandler>()
const questionHandlers = new Set<QuestionHandler>()
const statusHandlers = new Set<StatusHandler>()

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
