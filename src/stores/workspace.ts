// 工作区 UI 状态：活动会话、已打开标签、面板开关、模型与执行模式选择。
// 标签关闭不删除会话；删除会话时同步清理标签与活动状态。

import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { ExecutionMode } from '../types/session'

export const MODELS = [
  'GS/Qwen3-Coder-Next-FP8 · medium',
  'GS/Qwen3-Coder-Next-FP8 · large',
  'OpenCode / Claude Sonnet 5',
] as const

export const EXECUTION_MODES: { value: ExecutionMode; label: string }[] = [
  { value: 'auto', label: '自动' },
  { value: 'confirm', label: '需确认' },
  { value: 'plan', label: '计划模式' },
]

export const useWorkspaceStore = defineStore('workspace', () => {
  const activeSessionId = ref<string | null>(null)
  const openSessionIds = ref<string[]>([])
  const sidebarVisible = ref(true)
  const inspectorVisible = ref(true)
  /** 侧栏搜索框开关（侧栏隐藏时由顶栏搜索按钮联动打开） */
  const sidebarSearchOpen = ref(false)
  const selectedModel = ref<string>(MODELS[0])
  const executionMode = ref<ExecutionMode>('auto')
  const generatingSessionIds = ref<string[]>([])

  /** 打开会话标签（已打开则复用），并设为活动会话 */
  function openSession(id: string) {
    if (!openSessionIds.value.includes(id)) openSessionIds.value.push(id)
    activeSessionId.value = id
  }

  /** 关闭标签（不删除会话）；关闭活动标签时切换到相邻标签 */
  function closeTab(id: string) {
    const idx = openSessionIds.value.indexOf(id)
    if (idx === -1) return
    openSessionIds.value.splice(idx, 1)
    if (activeSessionId.value === id) {
      const next = openSessionIds.value[idx] ?? openSessionIds.value[idx - 1] ?? null
      activeSessionId.value = next
    }
  }

  /** 删除会话：同步清理标签与活动状态 */
  function forgetSession(id: string) {
    closeTab(id)
    openSessionIds.value = openSessionIds.value.filter((s) => s !== id)
    if (activeSessionId.value === id) activeSessionId.value = null
  }

  function setGenerating(id: string, generating: boolean) {
    const has = generatingSessionIds.value.includes(id)
    if (generating && !has) generatingSessionIds.value.push(id)
    if (!generating && has) generatingSessionIds.value = generatingSessionIds.value.filter((s) => s !== id)
  }

  function isGenerating(id: string | null): boolean {
    return !!id && generatingSessionIds.value.includes(id)
  }

  return {
    activeSessionId,
    openSessionIds,
    sidebarVisible,
    inspectorVisible,
    sidebarSearchOpen,
    selectedModel,
    executionMode,
    generatingSessionIds,
    openSession,
    closeTab,
    forgetSession,
    setGenerating,
    isGenerating,
  }
})
