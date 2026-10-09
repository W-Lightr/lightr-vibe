// 文件系统服务层：fs 命令与目录对话框的唯一入口（tauri-bridge 规范 §2）。
// isTauri() 双通道：Tauri 环境走 Rust invoke（真实磁盘），浏览器 dev 环境
// 整体回退 projectMock（内存树）。组件与 store 不直接感知通道差异。
// 项目边界：读写前校验 path 以项目 rootPath 为前缀（design §2，前端校验为主，
// Rust 侧再做 `..` 规范化兜底）。

import { invoke } from '@tauri-apps/api/core'
import type { DirEntry, FileContent, FileWriteResult } from '../types/project'
import { isTauri } from '../utils/isTauri'
import {
  mockListDir,
  mockPickFolder,
  mockReadFile,
  mockReadText,
  mockWriteFile,
  mockWriteText,
} from './projectMock'

interface ListDirResult {
  entries: DirEntry[]
}

/** 路径分隔符统一为 `/` 并去掉末尾斜杠，便于前缀比较（Windows 大小写不敏感） */
function canonical(path: string): string {
  return path.replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase()
}

/** path 是否在 root 子树内（含根本身）；用于项目边界前端校验 */
export function isInsideProject(path: string, root: string): boolean {
  const c = canonical(path)
  const r = canonical(root)
  return c === r || c.startsWith(`${r}/`)
}

export const fsService = {
  /** 原生目录选择对话框（design D1：前端 dialog 插件）；浏览器/取消返回 null */
  async pickFolder(): Promise<string | null> {
    if (!isTauri()) return mockPickFolder()
    const { open } = await import('@tauri-apps/plugin-dialog')
    const picked = await open({ directory: true, multiple: false })
    if (typeof picked === 'string') return picked
    return null
  },

  /** 列目录（懒加载）；root 为项目根路径（边界校验基准，双通道一致） */
  async listDir(path: string, root: string): Promise<DirEntry[]> {
    if (!isInsideProject(path, root)) throw new Error('outside-project')
    if (!isTauri()) return mockListDir(path)
    const result = await invoke<ListDirResult>('fs_list_dir', { path, rootPath: root })
    return result.entries
  },

  /** 读文本文件（>2MB / 二进制 / 越界返回约定错误串，双通道一致） */
  async readFile(path: string, root: string): Promise<FileContent> {
    if (!isInsideProject(path, root)) throw new Error('outside-project')
    if (!isTauri()) return mockReadFile(path)
    return invoke<FileContent>('fs_read_file', { path, rootPath: root })
  },

  /** 写文本文件；expectedMtime 为读取基线，不一致抛「conflict」（不静默覆盖） */
  async writeFile(
    path: string,
    text: string,
    expectedMtime: number | null,
    root: string,
  ): Promise<FileWriteResult> {
    if (!isInsideProject(path, root)) throw new Error('outside-project')
    if (!isTauri()) return mockWriteFile(path, text, expectedMtime)
    return invoke<FileWriteResult>('fs_write_file', { path, text, expectedMtime, rootPath: root })
  },

  /** 读应用自有文本文件（如 projects.json）；不存在返回 null，非错误 */
  async readText(path: string): Promise<string | null> {
    if (!isTauri()) return mockReadText(path)
    return invoke<string | null>('fs_read_text', { path })
  },

  /** 写应用自有文本文件（目录自动创建 + 原子写） */
  async writeText(path: string, text: string): Promise<void> {
    if (!isTauri()) {
      await mockWriteText(path, text)
      return
    }
    await invoke('fs_write_text', { path, text })
  },
}

/** 从目录名生成稳定的项目 ID（同一路径重复打开可复用；大小写不敏感） */
export function projectIdFromPath(rootPath: string): string {
  let hash = 0
  const s = canonical(rootPath)
  for (let i = 0; i < s.length; i++) {
    hash = (hash * 31 + s.charCodeAt(i)) | 0
  }
  return `proj-${(hash >>> 0).toString(36)}`
}

/** 目录绝对路径 → 显示名（末段目录名，保留原始大小写） */
export function projectNameFromPath(rootPath: string): string {
  const segs = rootPath.replace(/\\/g, '/').replace(/\/+$/, '').split('/').filter(Boolean)
  return segs[segs.length - 1] ?? rootPath
}

/** 是否支持原生目录选择（浏览器 dev 环境为 false，按钮呈禁用态） */
export function canPickFolder(): boolean {
  return isTauri()
}

/** 拼接目录与子项为绝对路径（保留原始分隔风格，统一用系统分隔符） */
export function joinPath(dir: string, name: string): string {
  const sep = dir.includes('\\') ? '\\' : '/'
  return `${dir.replace(/[\\/]+$/, '')}${sep}${name}`
}
