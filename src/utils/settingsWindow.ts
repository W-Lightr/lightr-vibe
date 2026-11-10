// 独立设置窗口：全应用唯一入口。
// why: 设置不再抢占主窗口路由 —— 桌面端打开（或聚焦）单例 settings-window，
// 浏览器 dev 返回 false 由调用方回退页内 /settings（PRD R1/R3）。
// 组件里不要直接 new WebviewWindow，统一走本模块（design §2）。

import { isTauri } from './isTauri'

export const SETTINGS_WINDOW_LABEL = 'settings-window'

/** 并发创建去重：双击设置按钮只应产生一个窗口 */
let inFlight: Promise<boolean> | null = null

/**
 * 打开（或聚焦）设置窗口。
 * @returns true = 桌面端已创建/聚焦设置窗口；false = 非 Tauri，调用方回退页内路由
 */
export async function openSettingsWindow(): Promise<boolean> {
  if (!isTauri()) return false
  if (inFlight) return inFlight

  inFlight = (async () => {
    // 动态导入：浏览器 dev 与非设置窗口路径不应把窗口 API 拉进首屏依赖
    const { WebviewWindow } = await import('@tauri-apps/api/webviewWindow')
    const existing = await WebviewWindow.getByLabel(SETTINGS_WINDOW_LABEL)
    if (existing) {
      // 已存在时只聚焦，不重复创建（AC2）
      await existing.unminimize().catch(() => undefined)
      await existing.show().catch(() => undefined)
      await existing.setFocus().catch(() => undefined)
      return true
    }

    const win = new WebviewWindow(SETTINGS_WINDOW_LABEL, {
      url: 'settings-window',
      title: '设置',
      width: 1100,
      height: 760,
      minWidth: 900,
      minHeight: 600,
      center: true,
      decorations: false,
      shadow: false,
    })
    await new Promise<void>((resolve, reject) => {
      void win.once('tauri://created', () => resolve())
      void win.once('tauri://error', (e) =>
        reject(new Error(String((e as { payload?: unknown }).payload ?? '窗口创建失败'))),
      )
    })
    return true
  })()

  try {
    return await inFlight
  } finally {
    inFlight = null
  }
}
