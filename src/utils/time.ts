// 统一的时间格式化工具

const pad = (n: number) => String(n).padStart(2, '0')

/** 绝对时间：YYYY-MM-DD HH:mm */
export function formatDateTime(ts: number): string {
  const d = new Date(ts)
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}

/** 相对时间：刚刚 / N 分钟前 / N 小时前 / N 天前 / 日期 */
export function formatRelativeTime(ts: number, now: number = Date.now()): string {
  const diff = Math.max(0, now - ts)
  const min = Math.floor(diff / 60_000)
  if (min < 1) return '刚刚'
  if (min < 60) return `${min}分钟前`
  const hour = Math.floor(min / 60)
  if (hour < 24) return `${hour}小时前`
  const day = Math.floor(hour / 24)
  if (day < 7) return `${day}天前`
  return formatDateTime(ts).slice(0, 10)
}

/** 时长：45秒 / 3分20秒 / 1小时2分 */
export function formatDuration(ms: number): string {
  const sec = Math.max(0, Math.floor(ms / 1000))
  if (sec < 60) return `${sec}秒`
  const min = Math.floor(sec / 60)
  if (min < 60) return `${min}分${sec % 60 ? `${sec % 60}秒` : ''}`
  const hour = Math.floor(min / 60)
  return `${hour}小时${min % 60 ? `${min % 60}分` : ''}`
}

/** Token 数量紧凑展示：980 / 1.2k / 3.4M */
export function formatTokens(n: number): string {
  if (n < 1000) return String(n)
  if (n < 1_000_000) return `${(n / 1000).toFixed(1)}k`
  return `${(n / 1_000_000).toFixed(1)}M`
}
