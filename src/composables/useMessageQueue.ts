// 会话消息队列（连续发送排队、适机发送）。设计见 design.md §3。
//
// 模块单例 + 按 sessionId 隔离，与 useAcpPermission / useAcpQuestion 同构：
// 队列是会话的运行期附属状态，切页/切标签回来必须还在，因此不能挂在组件里；
// 也不进 Pinia store，因为它不该被持久化。
//
// 单一数据源：模块级 Map 持有 `reactive` 容器本体（不是拷贝），UI 与 flush
// 读写同一份数组，避免「改了一个副本、另一个副本没同步」的双写问题。

import { reactive } from 'vue'
import { createId } from '../utils/id'

export interface QueuedMessage {
  id: string
  content: string
  createdAt: number
}

interface QueueState {
  items: QueuedMessage[]
}

// 每个会话一个 reactive 容器：直接改 items 即可让所有 useQueue 的调用方同步更新
const queues = new Map<string, QueueState>()

function stateOf(sessionId: string): QueueState {
  let state = queues.get(sessionId)
  if (!state) {
    state = reactive<QueueState>({ items: [] })
    queues.set(sessionId, state)
  }
  return state
}

/** 同步读取队列长度（发送路径当拍判断用，不经过响应式代理开销） */
export function getQueueLength(sessionId: string): number {
  return queues.get(sessionId)?.items.length ?? 0
}

/**
 * 尾部入队。空内容（trim 后）直接忽略并返回 null：
 * 队列里出现空串意味着 flush 时必然白跑一轮，还会被当成「发送成功」删掉。
 */
export function enqueue(
  sessionId: string,
  content: string,
  createdAt = Date.now(),
): QueuedMessage | null {
  const text = content.trim()
  if (!text) return null
  const item: QueuedMessage = { id: createId('quq'), content: text, createdAt }
  stateOf(sessionId).items.push(item)
  return item
}

/** 队首出队（flush 用）。队列为空返回 null */
export function dequeue(sessionId: string): QueuedMessage | null {
  const items = queues.get(sessionId)?.items
  if (!items || items.length === 0) return null
  return items.shift() ?? null
}

/**
 * 放回队首（flush 发送失败时用）：保留原 id/createdAt，退避后重试的是同一条。
 *
 * 幂等：若该 id 已在队列里（例如失败前已被别处放回），先摘掉原位置再插到队首，
 * 否则会出现同一条消息在队列里重复两次、被发两遍。
 */
export function requeueFront(sessionId: string, item: QueuedMessage): void {
  const items = stateOf(sessionId).items
  const idx = items.findIndex((m) => m.id === item.id)
  if (idx === 0) {
    items[0] = item
    return
  }
  if (idx > 0) items.splice(idx, 1)
  items.unshift(item)
}

/** 按 id 移除；返回是否真的移除了 */
export function remove(sessionId: string, id: string): boolean {
  const items = queues.get(sessionId)?.items
  if (!items) return false
  const idx = items.findIndex((m) => m.id === id)
  if (idx < 0) return false
  items.splice(idx, 1)
  return true
}

/** 上移一位（index <= 0 时无操作）；返回是否移动了 */
export function reorder(sessionId: string, id: string, delta: number): boolean {
  const items = queues.get(sessionId)?.items
  if (!items) return false
  const idx = items.findIndex((m) => m.id === id)
  if (idx < 0) return false
  const target = idx + delta
  if (target < 0 || target >= items.length) return false
  const [item] = items.splice(idx, 1)
  if (!item) return false
  items.splice(target, 0, item)
  return true
}

/** 移到队首（「优先发送」的降级实现，见 design.md §6）；已在队首返回 false */
export function moveToFront(sessionId: string, id: string): boolean {
  const items = queues.get(sessionId)?.items
  if (!items) return false
  const idx = items.findIndex((m) => m.id === id)
  if (idx <= 0) return false
  const [item] = items.splice(idx, 1)
  if (!item) return false
  items.unshift(item)
  return true
}

/**
 * 行内编辑：内容 trim 后为空视为非法，返回 false 且不改动。
 * 允许编辑成与原文相同的文本（不算失败）。
 */
export function update(sessionId: string, id: string, content: string): boolean {
  const text = content.trim()
  if (!text) return false
  const items = queues.get(sessionId)?.items
  const item = items?.find((m) => m.id === id)
  if (!item) return false
  item.content = text
  return true
}

/** 队列该被消费的通知（队列逻辑不碰发送链路，由 useConversation 注册） */
let flushHook: ((sessionId: string) => void) | null = null

/** 注册消费者（useConversation 初始化时调用，只保留最后一个注册者） */
export function onQueueFlushNeeded(hook: (sessionId: string) => void): void {
  flushHook = hook
}

/**
 * 请求一次消费。
 *
 * 存在的意义是消除一个竞态：用户直发时若 turn 恰好在这一拍收尾，finally 里的
 * 兜底 flush 会比「直发失败再入队」更早跑完，于是没人再来看队列。
 * 因此直发**之前**就先请求一次——入队后必然还有人来消费。
 */
export function requestFlush(sessionId: string): void {
  flushHook?.(sessionId)
}

/** 清空该会话队列 */
export function clear(sessionId: string): void {
  const state = queues.get(sessionId)
  if (!state) return
  state.items.splice(0, state.items.length)
}

/** 丢弃整个会话的队列容器（关闭连接时调用；之后再有入队会新建空容器） */
export function dropQueue(sessionId: string): void {
  queues.delete(sessionId)
}

/**
 * 组件侧读取：返回该会话的响应式列表。
 * 直接用 reactive 容器而不是拷贝，保证 UI 与 flush 看到同一份数据。
 */
export function useQueue(sessionId: string): { items: QueuedMessage[] } {
  return stateOf(sessionId)
}

