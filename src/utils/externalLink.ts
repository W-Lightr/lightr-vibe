// 外部链接统一入口：让应用内任何链接点击都在「外部」打开，
// 而不是让 Tauri WebView 就地导航（那会把整个软件界面替换成目标网页，AC10 实机缺陷）。
//
// - Tauri 桌面：tauri-plugin-opener 的 openUrl → 系统默认浏览器
// - 浏览器 dev：window.open 新标签页
//
// **唯一触发点**：main.ts 的全局 capture 监听。不要在各组件里再加局部点击处理，
// 否则同一次点击会被处理两次 → 开两个标签页。
import { isTauri } from './isTauri'

/** 以外部方式打开一个 URL。opener 不可用时静默忽略（不把错误抛进渲染层）。 */
export async function openExternal(url: string): Promise<void> {
  if (!url) return
  try {
    if (isTauri()) {
      const { openUrl } = await import('@tauri-apps/plugin-opener')
      await openUrl(url)
    } else {
      window.open(url, '_blank', 'noopener,noreferrer')
    }
  } catch {
    /* 忽略：打不开不应影响界面 */
  }
}

/** 从事件目标向上找最近的 <a href>，命中则外部打开并 preventDefault，返回是否处理。
 *  对同一事件做幂等标记，防止意外重复派发导致开多个标签。 */
export function handleExternalLinkClick(e: MouseEvent): boolean {
  const anchor = (e.target as HTMLElement | null)?.closest?.('a[href]') as
    | HTMLAnchorElement
    | null
  if (!anchor) return false
  const href = anchor.getAttribute('href')
  if (!href) return false
  if ((e as MouseEvent & { __lightrExternalHandled?: boolean }).__lightrExternalHandled) {
    return true
  }
  ;(e as MouseEvent & { __lightrExternalHandled?: boolean }).__lightrExternalHandled = true
  e.preventDefault()
  void openExternal(href)
  return true
}
