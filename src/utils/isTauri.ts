// Tauri 环境探测：供 service 层选择真实通道还是 mock（design §1 边界原则）。
// why: 前端需同时支持 `pnpm dev:tauri`（真实）与 `pnpm dev`（浏览器 mock），
// 只有 service 层知道当前环境，组件/store 保持无感。

/** 是否运行在 Tauri 桌面环境（注入 __TAURI_INTERNALS__ 即为真） */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}
