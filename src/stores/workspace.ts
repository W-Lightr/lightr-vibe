// 工作区 UI 状态：打开的标签（会话/文件）、活动标签、面板开关、模型与执行模式选择。
// 标签关闭不删除会话；删除会话时同步清理标签与活动状态。

import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type { ExecutionMode, OpenTab } from '../types/session'

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
  const tabs = ref<OpenTab[]>([])
  const activeTabId = ref<string | null>(null)
  const sidebarVisible = ref(true)
  const inspectorVisible = ref(true)
  /** 左/右栏宽度（px），拖拽分隔条调整 */
  const leftWidth = ref(260)
  const rightWidth = ref(335)
  /** 侧栏搜索框开关（侧栏隐藏时由顶栏搜索按钮联动打开） */
  const sidebarSearchOpen = ref(false)
  const selectedModel = ref<string>(MODELS[0])
  const executionMode = ref<ExecutionMode>('auto')
  const generatingSessionIds = ref<string[]>([])
  /** dirty 文件标签集合（tabId）：编辑器内容偏离加载基线，标签显示圆点 */
  const dirtyFileTabs = ref<string[]>([])

  const activeTab = computed(() => tabs.value.find((t) => t.id === activeTabId.value) ?? null)
  const activeSessionId = computed(() =>
    activeTab.value?.kind === 'session' ? (activeTab.value.sessionId ?? null) : null,
  )

  /** 打开会话标签（已打开则复用），并设为活动标签 */
  function openSession(sessionId: string) {
    let tab = tabs.value.find((t) => t.kind === 'session' && t.sessionId === sessionId)
    if (!tab) {
      tab = { id: `tab-ses-${sessionId}`, kind: 'session', sessionId }
      tabs.value.push(tab)
    }
    activeTabId.value = tab.id
  }

  /** 打开文件编辑标签（同路径复用） */
  function openFile(filePath: string, fileName: string, projectId: string) {
    let tab = tabs.value.find((t) => t.kind === 'file' && t.filePath === filePath)
    if (!tab) {
      tab = { id: `tab-file-${filePath}`, kind: 'file', filePath, fileName, projectId }
      tabs.value.push(tab)
    }
    activeTabId.value = tab.id
  }

  /** 关闭标签（不删除会话/文件）；关闭活动标签时切换到相邻标签；清理 dirty 标记 */
  function closeTab(tabId: string) {
    const idx = tabs.value.findIndex((t) => t.id === tabId)
    if (idx === -1) return
    tabs.value.splice(idx, 1)
    dirtyFileTabs.value = dirtyFileTabs.value.filter((t) => t !== tabId)
    if (activeTabId.value === tabId) {
      activeTabId.value = tabs.value[idx]?.id ?? tabs.value[idx - 1]?.id ?? null
    }
  }

  /** 删除会话：同步清理其标签与活动状态 */
  function forgetSession(sessionId: string) {
    tabs.value = tabs.value.filter((t) => !(t.kind === 'session' && t.sessionId === sessionId))
    if (!tabs.value.some((t) => t.id === activeTabId.value)) {
      activeTabId.value = tabs.value[0]?.id ?? null
    }
  }

  function setGenerating(id: string, generating: boolean) {
    const has = generatingSessionIds.value.includes(id)
    if (generating && !has) generatingSessionIds.value.push(id)
    if (!generating && has) {
      generatingSessionIds.value = generatingSessionIds.value.filter((s) => s !== id)
    }
  }

  function isGenerating(id: string | null | undefined): boolean {
    return !!id && generatingSessionIds.value.includes(id)
  }

  /** 设置文件标签 dirty 状态（tabId） */
  function setFileDirty(tabId: string, dirty: boolean) {
    const has = dirtyFileTabs.value.includes(tabId)
    if (dirty && !has) dirtyFileTabs.value.push(tabId)
    if (!dirty && has) dirtyFileTabs.value = dirtyFileTabs.value.filter((t) => t !== tabId)
  }

  function isFileDirty(tabId: string | null | undefined): boolean {
    return !!tabId && dirtyFileTabs.value.includes(tabId)
  }

  return {
    tabs,
    activeTabId,
    activeTab,
    activeSessionId,
    sidebarVisible,
    inspectorVisible,
    leftWidth,
    rightWidth,
    sidebarSearchOpen,
    selectedModel,
    executionMode,
    generatingSessionIds,
    dirtyFileTabs,
    openSession,
    openFile,
    closeTab,
    forgetSession,
    setGenerating,
    isGenerating,
    setFileDirty,
    isFileDirty,
  }
})
