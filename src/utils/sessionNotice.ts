// session/update `notice`（ACP schema 1.9 Session Notices）解码（唯一 owner）。
// 依据 codeg `lib/session-notices.ts` 的 `presentSessionNotice` / `splitHeadline`：
// - 无标题（首行空）→ 整条丢弃（返回 null，不弹空 toast）
// - severity 映射：warning / error 原样，其余（含缺失/未知）一律 info
// - 标题只取首个非空行；正文即 description（去掉与标题重复的首行）
// 该模块纯函数、无依赖，便于单测；消费面只有 useConversation.applyUpdate 的 notice 分支。

import type { AcpNoticeUpdateEvent } from '../types/acp'

/** toast 级别（与 naive-ui `message.*` 一一对应，不含 success） */
export type NoticeToastLevel = 'info' | 'warning' | 'error'

export interface NoticeToast {
  level: NoticeToastLevel
  /** 标题（单个非空行） */
  title: string
  /** 正文；空串表示无补充说明 */
  description: string
}

/** severity → toast 级别（未知一律降级为 info，不误报成警告） */
export function noticeLevel(severity: unknown): NoticeToastLevel {
  return severity === 'warning' || severity === 'error' ? severity : 'info'
}

/** 取首个非空行作为标题；无则 null */
export function splitHeadline(title: unknown): string | null {
  if (typeof title !== 'string') return null
  for (const line of title.split(/\r?\n/)) {
    const trimmed = line.trim()
    if (trimmed) return trimmed
  }
  return null
}

/**
 * notice → toast 描述；无标题返回 null（codeg 同样丢弃无标题通知，
 * 避免弹出只有正文、没有主体的空壳）。
 */
export function noticeToast(update: AcpNoticeUpdateEvent): NoticeToast | null {
  const title = splitHeadline(update.title)
  if (!title) return null
  const description = typeof update.description === 'string' ? update.description.trim() : ''
  return { level: noticeLevel(update.severity), title, description }
}
