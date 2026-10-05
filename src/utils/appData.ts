// 软件数据目录约定：用户目录下的 .lightr-vibe 为应用数据根目录，
// 其中 tmpsession/ 存放临时聊天会话数据。
// 当前仅解析路径供界面与后续持久化使用；真实读写待接入 Tauri fs 插件。

let cachedDir: string | null = null

export const APP_DATA_DIR_NAME = '.lightr-vibe'
export const TMP_SESSION_DIR_NAME = 'tmpsession'

/** 同步读取数据目录（未初始化时返回占位路径） */
export function appDataDirSync(): string {
  return cachedDir ?? `~/${APP_DATA_DIR_NAME}`
}

export function tmpSessionDirSync(): string {
  return `${appDataDirSync()}/${TMP_SESSION_DIR_NAME}`
}

/** 异步解析真实用户目录（Tauri 环境）；浏览器 dev 环境回退为占位路径 */
export async function initAppData(): Promise<string> {
  if (cachedDir) return cachedDir
  try {
    const { homeDir } = await import('@tauri-apps/api/path')
    const home = (await homeDir()).replace(/[\\/]+$/, '')
    cachedDir = `${home}/${APP_DATA_DIR_NAME}`
  } catch {
    cachedDir = `~/${APP_DATA_DIR_NAME}`
  }
  return cachedDir
}
