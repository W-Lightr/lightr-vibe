// 会话数据仓库：会话与消息的增删改查、流式更新、Token 统计。
// 生成控制（AbortController）在 useConversation 组合式函数中，避免 store 持有不可序列化状态。

import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type { Message, Session, ToolCall } from '../types/session'
import { conversationService } from '../services/conversationService'
import { createId } from '../utils/id'
import { tmpSessionDirSync } from '../utils/appData'

// 示例文件夹会话（演示「文件夹」分组与文件浏览；后续由真实目录接入）
function seedFolderSession(): Session {
  const now = Date.now()
  const s = conversationService.createSession('snake 游戏', 'GS/Qwen3-Coder-Next-FP8 · medium', {
    kind: 'folder',
    projectId: 'proj-29028',
  })
  s.createdAt = now - 20 * 3_600_000
  s.updatedAt = now - 17 * 3_600_000
  s.messages = [
    {
      id: createId('msg'),
      sessionId: s.id,
      role: 'user',
      type: 'text',
      content: '写一个贪吃蛇 html',
      status: 'done',
      createdAt: s.createdAt,
    },
    {
      id: createId('msg'),
      sessionId: s.id,
      role: 'assistant',
      type: 'text',
      content: '已创建贪吃蛇游戏： snake.html\n\n使用方向键控制，撞墙或撞到自己则重新开始。',
      status: 'done',
      createdAt: s.createdAt + 60_000,
    },
  ]
  return s
}

export const useSessionStore = defineStore('session', () => {
  const sessions = ref<Session[]>([seedFolderSession()])

  const byId = computed(() => {
    const map = new Map<string, Session>()
    for (const s of sessions.value) map.set(s.id, s)
    return map
  })

  function get(id: string | null | undefined): Session | null {
    return id ? (byId.value.get(id) ?? null) : null
  }

  function createSession(
    model: string,
    opts: { kind: Session['kind']; projectId?: string },
  ): Session {
    const session = conversationService.createSession(
      `新会话 ${sessions.value.length + 1}`,
      model,
      opts,
    )
    // 临时会话落盘路径：用户数据目录 /.lightr-vibe/tmpsession/<id>
    if (session.kind === 'tmp') session.storagePath = `${tmpSessionDirSync()}/${session.id}`
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
