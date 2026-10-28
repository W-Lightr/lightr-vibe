// 会话消息队列的两个纯判定函数（设计见 design.md §4.4 / §5）。
// 单独成文件是为了可离线验证：它们不含状态、不碰 DOM/ACP，
// 「什么时候该入队」「退避还要等多久」这类判断错了会导致消息丢失或重复发送。

/**
 * 用户直发时是否需要改为「尾部入队」。
 *
 * - 自动 flush 发起的发送永远直发（它本身就代表队首，再入队会死循环）。
 * - 队列非空时，用户直发必须排队，否则会插到已排队的消息之前，破坏 FIFO。
 * - 生成中不在这里判断：那是 `send` 的返回契约（'would-queue'），
 *   保证底层函数不会替调用方偷偷入队。
 */
export function shouldQueueDirectSend(fromFlush: boolean, queueLength: number): boolean {
  if (fromFlush) return false
  return queueLength > 0
}

/**
 * 一次 flush 失败后，下次允许重试前还要等多少毫秒。
 *
 * 为什么要退避而不是立刻重试：send 失败多半是连接/门禁层面的瞬时问题，
 * 立刻重试会在同一拍里反复取队首、反复失败，形成忙等。窗口内返回剩余时间，
 * 窗口外返回 0（可以立刻重试）。
 *
 * @param now          当前时间戳（ms）
 * @param lastBounceAt 上次失败的时间戳（ms）；0 表示从未失败
 * @param windowMs     退避窗口长度（ms）
 */
export function flushRetryDelayMs(now: number, lastBounceAt: number, windowMs = 1000): number {
  if (!lastBounceAt) return 0
  const elapsed = now - lastBounceAt
  if (elapsed >= windowMs) return 0
  // 时钟回拨（elapsed < 0）时同样按「窗口内」处理，返回整窗，避免负数延迟
  return Math.min(windowMs, windowMs - Math.max(0, elapsed))
}
