// 会话数据仓库：会话与消息的增删改查、流式更新、Token 统计。
// 生成控制（AbortController）在 useConversation 组合式函数中，避免 store 持有不可序列化状态。

import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type { Message, Session, ToolCall } from '../types/session'
import { conversationService } from '../services/conversationService'
import { createId } from '../utils/id'

export const useSessionStore = defineStore('session', () => {
  const sessions = ref<Session[]>([])

  const byId = computed(() => {
    const map = new Map<string, Session>()
    for (const s of sessions.value) map.set(s.id, s)
    return map
  })

  function get(id: string | null | undefined): Session | null {
    return id ? (byId.value.get(id) ?? null) : null
  }

  function createSession(model: string): Session {
    const session = conversationService.createSession(`新会话 ${sessions.value.length + 1}`, model)
    sessions.value.unshift(session)
    return session
  }

  function renameSession(id: string, title: string) {
    const s = get(id)
    if (!s) return
    s.title = title.trim() || s.title
    s.updatedAt = Date.now()
  }

  function deleteSession(id: string) {
    const idx = sessions.value.findIndex((s) => s.id === id)
    if (idx !== -1) sessions.value.splice(idx, 1)
  }

  function touch(id: string) {
    const s = get(id)
    if (s) s.updatedAt = Date.now()
  }

  // ---- 消息 ----

  function pushMessage(
    sessionId: string,
    msg: Omit<Message, 'id' | 'sessionId' | 'createdAt'> & { id?: string; createdAt?: number },
  ): Message {
    const s = get(sessionId)
    const message: Message = {
      ...msg,
      id: msg.id ?? createId('msg'),
      sessionId,
      createdAt: msg.createdAt ?? Date.now(),
    }
    s?.messages.push(message)
    return message
  }

  function messageById(sessionId: string, messageId: string): Message | undefined {
    return get(sessionId)?.messages.find((m) => m.id === messageId)
  }

  function appendContent(sessionId: string, messageId: string, delta: string) {
    const m = messageById(sessionId, messageId)
    if (m) m.content += delta
  }

  function patchMessage(sessionId: string, messageId: string, patch: Partial<Message>) {
    const m = messageById(sessionId, messageId)
    if (m) Object.assign(m, patch)
  }

  function patchToolCall(sessionId: string, messageId: string, toolCallId: string, patch: Partial<ToolCall>) {
    const tc = messageById(sessionId, messageId)?.toolCalls?.find((t) => t.id === toolCallId)
    if (tc) Object.assign(tc, patch)
  }

  // ---- Token 统计 ----

  function addUsage(sessionId: string, usage: { input: number; output: number }) {
    const s = get(sessionId)
    if (!s) return
    s.tokenUsage.input += usage.input
    s.tokenUsage.output += usage.output
    s.tokenUsage.total = s.tokenUsage.input + s.tokenUsage.output
  }

  function setStatus(sessionId: string, status: Session['status']) {
    const s = get(sessionId)
    if (s) s.status = status
  }

  return {
    sessions,
    get,
    createSession,
    renameSession,
    deleteSession,
    touch,
    pushMessage,
    messageById,
    appendContent,
    patchMessage,
    patchToolCall,
    addUsage,
    setStatus,
  }
})
