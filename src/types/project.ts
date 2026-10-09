// 项目（真实文件夹工作区）与目录条目模型。
// 项目持久化于 ~/.lightr-vibe/lightr-vibe/projects.json（父任务命名空间约束：
// 禁止触碰 ~/.lightr-vibe 根下其他应用的既有文件）。

/** 用户打开的真实文件夹项目 */
export interface Project {
  id: string
  /** 目录名（显示用） */
  name: string
  /** 项目根目录的绝对路径（fs 命令的项目边界基准） */
  rootPath: string
  /** 加入项目列表的时间（ms） */
  addedAt: number
  /** 最近一次打开/激活时间（ms），「最近」分组按其倒序 */
  lastOpenedAt: number
}

/** `fs_list_dir` 返回的目录条目（与 Rust DirEntry camelCase 契约一致） */
export interface DirEntry {
  name: string
  type: 'dir' | 'file'
  /** 文件字节数；目录为 undefined */
  size?: number
  mtimeMs?: number
}

/** `fs_read_file` 返回值 */
export interface FileContent {
  text: string
  mtimeMs: number
}

/** `fs_write_file` 返回值 */
export interface FileWriteResult {
  mtimeMs: number
}

/**
 * fs 命令约定错误串 → 中文提示映射的键。
 * Rust 侧错误串：too-large / binary / not-found / outside-project / conflict，
 * 其余为 io 错误消息（原样透传）。
 */
export type FsErrorKind = 'too-large' | 'binary' | 'not-found' | 'outside-project' | 'conflict'

export const FS_ERROR_MESSAGES: Record<FsErrorKind, string> = {
  'too-large': '文件超过 2MB，暂不支持打开',
  binary: '二进制文件，无法以文本方式打开',
  'not-found': '文件或目录不存在',
  'outside-project': '路径超出项目范围',
  conflict: '文件已被外部修改',
}

/** 把 Rust 错误串归类；非约定错误返回 null（调用方直接显示原文） */
export function classifyFsError(err: unknown): FsErrorKind | null {
  const msg = err instanceof Error ? err.message : String(err ?? '')
  // Tauri invoke 的 Err(String) 到达前端即为 message 本身
  const known: FsErrorKind[] = ['too-large', 'binary', 'not-found', 'outside-project', 'conflict']
  for (const kind of known) {
    if (msg === kind || msg.includes(kind)) return kind
  }
  return null
}

/** 错误串 → 用户可读中文提示 */
export function fsErrorMessage(err: unknown): string {
  const kind = classifyFsError(err)
  if (kind) return FS_ERROR_MESSAGES[kind]
  const msg = err instanceof Error ? err.message : String(err ?? '')
  return msg || '文件操作失败'
}
