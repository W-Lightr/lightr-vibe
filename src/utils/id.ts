/** 生成带前缀的稳定唯一 ID */
export function createId(prefix = 'id'): string {
  const c = globalThis.crypto
  if (c && typeof c.randomUUID === 'function') {
    return `${prefix}-${c.randomUUID().slice(0, 8)}`
  }
  return `${prefix}-${Math.random().toString(36).slice(2, 10)}${Date.now().toString(36).slice(-4)}`
}
