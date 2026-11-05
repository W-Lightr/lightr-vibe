// 编码工作区重依赖（Monaco 编辑器 / xterm 终端）的懒加载与预取入口。
// 动态 import 只在此处声明，组件与触发点统一调用，避免每个入口各写一份。
// 两个模块合计数 MB，绝不能被 CodingWorkspace 首屏 chunk 静态拉入。

export function loadFileEditor(): Promise<typeof import('./components/FileEditorView.vue')> {
  return import('./components/FileEditorView.vue')
}

export function loadTerminal(): Promise<typeof import('./components/TerminalPanel.vue')> {
  return import('./components/TerminalPanel.vue')
}

// 变更页签的 diff 视图同样依赖 Monaco：只在切到「变更」时加载。
export function loadDiffViewer(): Promise<typeof import('./components/DiffViewer.vue')> {
  return import('./components/DiffViewer.vue')
}

// 模块级 Promise 去重：同一 chunk 的预取只调度一次（动态 import 自身也有缓存，
// 这里主要避免重复的 requestIdleCallback / 事件反复触发）。
let fileEditorPrefetch: Promise<unknown> | null = null
let terminalPrefetch: Promise<unknown> | null = null

/** 预取失败静默吞掉：本地 chunk 缺失不应产生未处理 rejection，渲染时由 error fallback 兜底 */
function ignoreFailure(pending: Promise<unknown>): void {
  void pending.catch(() => undefined)
}

/** 空闲预取编辑器（含 Monaco）：进入工作区首帧之后调用 */
export function prefetchFileEditor(): void {
  if (!fileEditorPrefetch) fileEditorPrefetch = loadFileEditor()
  ignoreFailure(fileEditorPrefetch)
}

/** 预取终端（含 xterm）：仅在用户悬停/聚焦终端入口时调用，避免无谓加载 */
export function prefetchTerminal(): void {
  if (!terminalPrefetch) terminalPrefetch = loadTerminal()
  ignoreFailure(terminalPrefetch)
}

/**
 * 在浏览器空闲时执行任务，不可用时退化为 setTimeout。
 * @param task 待执行任务
 * @param timeoutMs requestIdleCallback 的最迟等待时间（ms）
 */
export function scheduleIdle(task: () => void, timeoutMs = 2000): void {
  if (typeof window.requestIdleCallback === 'function') {
    window.requestIdleCallback(() => task(), { timeout: timeoutMs })
    return
  }
  window.setTimeout(task, 200)
}
