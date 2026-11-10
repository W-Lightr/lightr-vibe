// 窗口句柄安全获取：Tauri 环境返回当前窗口，浏览器 dev 环境返回 null。
// why: getCurrentWindow() 依赖 window.__TAURI_INTERNALS__，纯浏览器中直接抛错；
// 设置页的窗口三键需按环境隐藏（PRD R3 / AC7 浏览器可用）。
// 模块加载时求值一次并缓存，避免每次渲染重复 try/catch。

import { getCurrentWindow, type Window } from '@tauri-apps/api/window'

let resolved: Window | null
try {
  resolved = getCurrentWindow()
} catch {
  resolved = null
}

/** 获取当前窗口句柄；浏览器 dev 环境返回 null（调用方隐藏窗口控件） */
export function getAppWindow(): Window | null {
  return resolved
}

/** 当前窗口 label；浏览器 dev 环境返回 null（用于启动时按窗口分流） */
export function getAppWindowLabel(): string | null {
  return resolved?.label ?? null
}
