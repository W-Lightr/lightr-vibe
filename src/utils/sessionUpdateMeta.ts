// session_info_update 的 `_meta` 解码（唯一 owner，避免组件/store 各自解析）。
// 依据 research/update-kinds.md：opencode 用 `_meta["opencode/retry"]`
// （{attempt, nextRetryAt, error:{type,message,status?}}），codex 用
// `_meta.codex.error`（仅在 willRetry === true 时为可重试）。形状未知一律返回
// null（宽容：不报错、不误渲染，PRD D3）。

import type { SessionRetryState } from '../types/session'

/**
 * 该 `_meta` 是否**携带重试信息**（opencode `opencode/retry` 键存在，或 codex
 * `codex.error` 键存在）。用于区分「无重试字段（应保持现状，勿误清）」与
 * 「显式重试=null/对象（应更新）」——`session_info_update` 也会带 `title` /
 * `opencode/compaction` 等无关 `_meta`，若一律 setRetry 会误清正在显示的重试横幅
 * （design §3.2：仅 retry=null 或 prompt 响应到达才清除）。
 */
export function hasRetryMeta(meta: unknown): boolean {
  if (!meta || typeof meta !== 'object') return false
  const obj = meta as Record<string, unknown>
  if ('opencode/retry' in obj) return true
  const codex = obj.codex as Record<string, unknown> | undefined
  return !!codex && 'error' in codex
}

/** 从任意 `_meta` 里提取可重试状态；不可重试/形状未知 → null（含 retry=null 清除语义） */
export function parseRetryState(meta: unknown): SessionRetryState | null {
  if (!meta || typeof meta !== 'object') return null
  const obj = meta as Record<string, unknown>

  // opencode：`_meta["opencode/retry"]`。显式 null → 清除；对象 → 解析
  const opencodeRetry = obj['opencode/retry']
  if (opencodeRetry === null) return null
  if (opencodeRetry && typeof opencodeRetry === 'object') {
    const r = opencodeRetry as Record<string, unknown>
    const err = (r.error ?? null) as Record<string, unknown> | null
    return {
      attempt: typeof r.attempt === 'number' ? r.attempt : undefined,
      nextRetryAt: typeof r.nextRetryAt === 'number' ? r.nextRetryAt : undefined,
      message: err && typeof err.message === 'string' ? err.message : undefined,
    }
  }

  // codex：`_meta.codex.error`，仅 willRetry === true 才算可重试
  const codex = obj.codex as Record<string, unknown> | undefined
  const codexErr = codex?.error as Record<string, unknown> | undefined
  if (codexErr && codexErr.willRetry === true) {
    return {
      message:
        typeof codexErr.message === 'string'
          ? codexErr.message
          : typeof codexErr.type === 'string'
            ? codexErr.type
            : undefined,
    }
  }

  return null
}
