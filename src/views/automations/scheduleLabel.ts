// cron + 时区 → 人话（纯函数，无 Vue 依赖，可单测）。
//
// 覆盖 UI 里最常见的几种形态；认不出的表达式原样回显，
// 而不是猜一个可能错的描述。

const WEEKDAYS = ['周日', '周一', '周二', '周三', '周四', '周五', '周六']

function num(raw: string): number | null {
  return /^\d+$/.test(raw) ? Number(raw) : null
}

function pad(n: number): string {
  return String(n).padStart(2, '0')
}

/** 简单步长描述：`*​/15` → 「每 15 分钟」 */
function everyStep(raw: string, unit: string): string | null {
  const m = /^\*\/(\d+)$/.exec(raw)
  if (!m) return null
  return `每 ${m[1]} ${unit}`
}

/**
 * cron → 中文描述。
 *
 * 支持（其余原样返回 cron 字符串）：
 * - `* * * * *`           每分钟
 * - `一般步长`              每 N 分钟 / 每 N 小时
 * - `M H * * *`            每天 HH:MM
 * - `M H * * D[,D…]`       每周 X…
 * - `M H D * *`            每月 D 号 HH:MM
 * - `M H * M *`            每年 M 月…
 */
export function scheduleLabel(cron: string, timezone: string): string {
  const trimmed = cron.trim()
  const fields = trimmed.split(/\s+/)
  if (fields.length !== 5) return trimmed || '未设置'
  const [minRaw, hourRaw, domRaw, monRaw, dowRaw] = fields

  const suffix = timezone ? ` · ${timezone}` : ''

  const minStep = everyStep(minRaw, '分钟')
  if (minStep && hourRaw === '*' && domRaw === '*' && monRaw === '*' && dowRaw === '*') {
    return minStep + suffix
  }
  const hourStep = everyStep(hourRaw, '小时')
  if (hourStep && minRaw === '0' && domRaw === '*' && monRaw === '*' && dowRaw === '*') {
    return hourStep + suffix
  }

  const min = minRaw === '*' ? null : num(minRaw)
  const hour = hourRaw === '*' ? null : num(hourRaw)

  if (min !== null && hour !== null) {
    const time = `${pad(hour)}:${pad(min)}`

    // 每周：dow 非通配
    if (dowRaw !== '*' && domRaw === '*' && monRaw === '*') {
      const days = dowRaw
        .split(',')
        .map((raw) => {
          const n = num(raw.trim())
          if (n === null) return null
          // UI 语义 0/7=周日
          return WEEKDAYS[n === 7 ? 0 : n] ?? null
        })
        .filter((d): d is string => d !== null)
      if (days.length > 0) return `每${days.join('、')} ${time}${suffix}`
    }

    // 每月：dom 非通配
    if (domRaw !== '*' && dowRaw === '*' && monRaw === '*') {
      const day = num(domRaw)
      if (day !== null) return `每月 ${day} 日 ${time}${suffix}`
    }

    // 每年：mon 非通配
    if (monRaw !== '*' && domRaw !== '*' && dowRaw === '*') {
      const month = num(monRaw)
      const day = num(domRaw)
      if (month !== null && day !== null) {
        return `每年 ${month} 月 ${day} 日 ${time}${suffix}`
      }
    }

    // 每天
    if (domRaw === '*' && monRaw === '*' && dowRaw === '*') {
      return `每天 ${time}${suffix}`
    }

    // 每小时的第 M 分
    if (hourRaw === '*' && domRaw === '*' && monRaw === '*' && dowRaw === '*') {
      return `每小时第 ${min} 分${suffix}`
    }
  }

  return `${trimmed}${suffix}`
}

/** 触发方式的中文描述（手动 / 定时 + 下次时刻） */
export function triggerLabel(
  triggerKind: 'schedule' | 'manual',
  cron: string,
  timezone: string,
): string {
  if (triggerKind === 'manual') return '仅手动触发'
  return scheduleLabel(cron, timezone)
}

/** 常用 cron 预设（CronBuilderDialog 与编辑器快捷按钮共用） */
export const CRON_PRESETS: { label: string; cron: string }[] = [
  { label: '每 5 分钟', cron: '*/5 * * * *' },
  { label: '每小时整点', cron: '0 * * * *' },
  { label: '每天 09:00', cron: '0 9 * * *' },
  { label: '每天 18:00', cron: '0 18 * * *' },
  { label: '每周一 09:00', cron: '0 9 * * 1' },
  { label: '工作日 09:00', cron: '0 9 * * 1-5' },
  { label: '每月 1 日 09:00', cron: '0 9 1 * *' },
]
