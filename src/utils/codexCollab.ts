// Codex ACP 的 `collabAgentToolCall` 是专有展示信号：它不创建本地子会话，
// 但携带的三元字段足以安全映射到委派卡片。字段变化或不完整时返回 null，
// 调用方保持普通工具调用渲染，避免把一般工具误判为子代理。

import type { DelegationStatus } from '../types/delegation'

export interface CodexCollabAgent {
  threadId: string
  status: string | null
  message: string | null
}

export interface CodexCollabInfo {
  prompt: string | null
  status: DelegationStatus
  model: string | null
  reasoningEffort: string | null
  agents: CodexCollabAgent[]
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null
}

function asText(value: unknown): string | null {
  return typeof value === 'string' && value.trim() ? value.trim() : null
}

/** Codex 原始状态映射为本应用已有的闭合委派状态。 */
export function codexCollabStatus(raw: string | null): DelegationStatus {
  switch (raw?.trim()) {
    case 'pendingInit':
    case 'running':
    case 'inProgress':
      return 'running'
    case 'completed':
      return 'completed'
    case 'shutdown':
    case 'interrupted':
      return 'interrupted'
    case 'errored':
    case 'failed':
    case 'notFound':
      return 'failed'
    default:
      return 'running'
  }
}

/**
 * 解析 codex-acp 1.0.1+ 映射的 collabAgentToolCall `rawInput`：
 * `{ prompt, senderThreadId, receiverThreadIds, agentsStates, status }`。
 */
export function parseCodexCollabInput(rawInput: string | undefined): CodexCollabInfo | null {
  if (!rawInput) return null
  let parsed: unknown
  try {
    parsed = JSON.parse(rawInput)
  } catch {
    return null
  }
  const input = asRecord(parsed)
  if (!input || !('senderThreadId' in input) || !('receiverThreadIds' in input) || !('agentsStates' in input)) {
    return null
  }

  const agents: CodexCollabAgent[] = []
  const states = asRecord(input.agentsStates)
  if (states) {
    for (const [threadId, state] of Object.entries(states)) {
      const entry = asRecord(state)
      if (!entry) continue
      agents.push({ threadId, status: asText(entry.status), message: asText(entry.message) })
    }
  }

  // 子任务失败必须优先于 collab op 的旧 running/completed 状态；其余取最后一个
  // 可用子状态，覆盖 spawn 阶段常见的 pendingInit。
  const statuses = agents.map((agent) => agent.status).filter((value): value is string => value !== null)
  const terminalFailure = statuses.find((value) => codexCollabStatus(value) === 'failed')
  const latest = terminalFailure ?? statuses[statuses.length - 1] ?? asText(input.status)
  return {
    prompt: asText(input.prompt),
    status: codexCollabStatus(latest),
    model: asText(input.model),
    reasoningEffort: asText(input.reasoningEffort),
    agents,
  }
}
