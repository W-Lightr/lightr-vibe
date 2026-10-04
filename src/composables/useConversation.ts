// 会话生成控制：发送消息、流式更新、停止生成。
// 每个会话独立的 AbortController，流式更新只影响对应会话。

import { conversationService, type MockEvent } from '../services/conversationService'
import { useSessionStore } from '../stores/session'
import { useWorkspaceStore } from '../stores/workspace'

const controllers = new Map<string, AbortController>()

export function useConversation() {
  const sessionStore = useSessionStore()
  const workspace = useWorkspaceStore()

  function applyEvent(sessionId: string, turnId: string, event: MockEvent) {
    switch (event.kind) {
      case 'thinking-start':
        sessionStore.pushMessage(sessionId, {
          id: event.messageId,
          role: 'assistant',
          type: 'thinking',
          content: '',
          status: 'streaming',
          turnId,
        })
        break
      case 'thinking-delta':
        sessionStore.appendContent(sessionId, event.messageId, event.delta)
        break
      case 'thinking-end':
        sessionStore.patchMessage(sessionId, event.messageId, { status: 'done' })
        break
      case 'tool-start':
        sessionStore.pushMessage(sessionId, {
          id: event.messageId,
          role: 'assistant',
          type: 'text',
          content: '',
          status: 'streaming',
          turnId,
          toolCalls: [event.toolCall],
        })
        break
      case 'tool-end':
        sessionStore.patchToolCall(sessionId, event.messageId, event.toolCallId, {
          status: 'done',
          result: event.result,
        })
        sessionStore.patchMessage(sessionId, event.messageId, { status: 'done' })
        break
      case 'reply-start':
        sessionStore.pushMessage(sessionId, {
          id: event.messageId,
          role: 'assistant',
          type: 'text',
          content: '',
          status: 'streaming',
          turnId,
        })
        break
      case 'reply-end':
        sessionStore.patchMessage(sessionId, event.messageId, { status: 'done' })
        sessionStore.addUsage(sessionId, event.usage)
        break
      case 'error':
        sessionStore.pushMessage(sessionId, {
          role: 'assistant',
          type: 'error',
          content: event.message,
          status: 'error',
          turnId,
        })
        break
    }
  }

  async function send(sessionId: string, content: string) {
    if (!content.trim() || workspace.isGenerating(sessionId)) return

    const turnId = `turn-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`
    sessionStore.pushMessage(sessionId, { role: 'user', type: 'text', content, status: 'done', turnId })
    sessionStore.setStatus(sessionId, 'generating')
    workspace.setGenerating(sessionId, true)

    const controller = new AbortController()
    controllers.set(sessionId, controller)

    try {
      await conversationService.send({
        sessionId,
        content,
        model: workspace.selectedModel,
        signal: controller.signal,
        onDelta: (messageId, delta) => sessionStore.appendContent(sessionId, messageId, delta),
        onEvent: (event) => applyEvent(sessionId, turnId, event),
      })
    } catch (err) {
      if ((err as DOMException)?.name === 'AbortError') {
        // 停止生成：把该回合中仍在流式的消息标记为已停止
        for (const m of sessionStore.get(sessionId)?.messages ?? []) {
          if (m.turnId === turnId && m.status === 'streaming') {
            sessionStore.patchMessage(sessionId, m.id, { status: 'stopped' })
          }
        }
      } else {
        sessionStore.setStatus(sessionId, 'error')
      }
    } finally {
      controllers.delete(sessionId)
      workspace.setGenerating(sessionId, false)
      sessionStore.setStatus(sessionId, 'idle')
      sessionStore.touch(sessionId)
    }
  }

  function stop(sessionId: string) {
    controllers.get(sessionId)?.abort()
  }

  /** 重新生成：移除该回合的助手侧消息，重发同回合的用户消息 */
  function regenerate(sessionId: string, assistantMessageId: string) {
    const session = sessionStore.get(sessionId)
    const target = session?.messages.find((m) => m.id === assistantMessageId)
    if (!session || !target?.turnId || workspace.isGenerating(sessionId)) return
    const turnId = target.turnId
    const userMsg = [...session.messages].reverse().find((m) => m.turnId === turnId && m.role === 'user')
    session.messages = session.messages.filter((m) => m.turnId !== turnId || m.role === 'user')
    if (userMsg) void send(sessionId, userMsg.content)
  }

  return { send, stop, regenerate }
}
