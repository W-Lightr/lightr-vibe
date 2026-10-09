// 浏览器 dev 环境的项目 mock（design §4）：内存目录树 + 内存读写，
// 形状适配真实通道的 DirEntry/FileContent 契约，组件经 fsService 统一消费。
// 浏览器无原生目录选择对话框：pickFolder 返回 null（「打开文件夹」按钮呈禁用态）。

import type { DirEntry, FileContent, FileWriteResult, Project } from '../types/project'

interface MockNode {
  name: string
  type: 'dir' | 'file'
  children?: MockNode[]
  /** 文件内容（mock 内存文件系统） */
  content?: string
  /** 文件修改时间（模拟冲突检测基线） */
  mtimeMs?: number
}

/** mock 项目：rootPath 形状（与真实通道一致，组件不感知双形状） */
const MOCK_ROOT = 'C:/Users/mock/29028'

const mockTree: Record<string, MockNode[]> = {
  [MOCK_ROOT]: [
    {
      name: '.ccgui',
      type: 'dir',
      children: [
        {
          name: 'config.json',
          type: 'file',
          content: '{\n  "theme": "light",\n  "language": "zh-CN"\n}\n',
          mtimeMs: 1,
        },
      ],
    },
    {
      name: 'snake.html',
      type: 'file',
      content:
        '<!doctype html>\n<html lang="zh-CN">\n  <head>\n    <meta charset="UTF-8" />\n    <title>贪吃蛇</title>\n  </head>\n  <body>\n    <canvas id="board" width="400" height="400"></canvas>\n    <script>\n      // 使用方向键控制，撞墙或撞到自己则重新开始\n    </script>\n  </body>\n</html>\n',
      mtimeMs: 2,
    },
    {
      name: 'super-mario.html',
      type: 'file',
      content:
        '<!doctype html>\n<html lang="zh-CN">\n  <head>\n    <meta charset="UTF-8" />\n    <title>Super Mario</title>\n  </head>\n  <body>\n    <div id="game"></div>\n  </body>\n</html>\n',
      mtimeMs: 3,
    },
    {
      name: '春天作文.txt',
      type: 'file',
      content: '春天来了，万物复苏。\n小草从土里探出头来，桃花也开了。\n',
      mtimeMs: 4,
    },
  ],
}

/** 应用自有文本文件（projects.json 等）的 mock 存储：内存 Map，刷新即重置 */
const mockTextFiles = new Map<string, string>()

/** mock 通道的项目列表种子（store 首次加载无持久化文件时使用）。
 *  id 与 session store 种子会话的 projectId（proj-29028）对齐，
 *  浏览器 demo 下既有会话仍归属该 mock 项目。 */
export function mockSeedProjects(): Project[] {
  const now = Date.now()
  return [
    {
      id: 'proj-29028',
      name: '29028',
      rootPath: MOCK_ROOT,
      addedAt: now - 86_400_000,
      lastOpenedAt: now - 3_600_000,
    },
  ]
}

/** 按绝对路径定位 mock 节点；根目录自身或不存在均返回 null */
function findNode(absPath: string): MockNode | null {
  for (const [root, children] of Object.entries(mockTree)) {
    if (!absPath.startsWith(root)) continue
    const rel = absPath.slice(root.length).replace(/^[\\/]+/, '')
    if (!rel) return null
    let level: MockNode[] | undefined = children
    let node: MockNode | undefined
    for (const seg of rel.split(/[\\/]+/).filter(Boolean)) {
      node = level?.find((n) => n.name === seg)
      if (!node) return null
      level = node.children
    }
    return node ?? null
  }
  return null
}

/** 目录子节点列表（根目录返回树顶层；.git 与真实通道一致返回空） */
function childrenOf(absPath: string): MockNode[] | null {
  const segments = absPath.split(/[\\/]+/).filter(Boolean)
  if (segments[segments.length - 1] === '.git') return []
  const node = findNode(absPath)
  if (node) return node.type === 'dir' ? (node.children ?? []) : null
  return mockTree[absPath] ?? null
}

/** 列目录：dirs-first + 名称排序（模拟真实契约与错误串） */
export function mockListDir(path: string): DirEntry[] {
  const children = childrenOf(path)
  if (!children) throw new Error('not-found')
  const entries: DirEntry[] = children.map((n) => ({
    name: n.name,
    type: n.type,
    size: n.type === 'file' ? (n.content?.length ?? 0) : undefined,
    mtimeMs: n.mtimeMs,
  }))
  return entries.sort((a, b) => {
    if (a.type !== b.type) return a.type === 'dir' ? -1 : 1
    return a.name.localeCompare(b.name)
  })
}

/** 读文件：模拟 too-large / binary / not-found 错误串 */
export function mockReadFile(path: string): FileContent {
  const node = findNode(path)
  if (!node || node.type !== 'file') throw new Error('not-found')
  const content = node.content ?? ''
  if (content.length > 2 * 1024 * 1024) throw new Error('too-large')
  if (content.includes('\u0000')) throw new Error('binary')
  return { text: content, mtimeMs: node.mtimeMs ?? 0 }
}

/** 写文件：conflict 四象限与 Rust fs_write_file 对齐——
 *  (null, 存在)=外部新建冲突；(非 null, 缺失)=状态漂移冲突；mtime 不一致=冲突 */
export function mockWriteFile(
  path: string,
  text: string,
  expectedMtime: number | null,
): FileWriteResult {
  const node = findNode(path)
  if (!node || node.type !== 'file') {
    // 读时存在、写时缺失 → 漂移冲突（与 Rust 一致）；mock 不支持新建文件
    if (expectedMtime !== null) throw new Error('conflict')
    throw new Error('not-found')
  }
  // 读时不存在、写时出现了 → 外部新建冲突（与 Rust 一致）
  if (expectedMtime === null) throw new Error('conflict')
  if (node.mtimeMs !== expectedMtime) throw new Error('conflict')
  node.content = text
  node.mtimeMs = (node.mtimeMs ?? 0) + 1
  return { mtimeMs: node.mtimeMs }
}

/** 浏览器无原生目录对话框：返回 null（UI 按钮禁用态兜底） */
export function mockPickFolder(): Promise<string | null> {
  return Promise.resolve(null)
}

/** 读应用自有文本文件（不存在返回 null，同真实契约） */
export function mockReadText(path: string): Promise<string | null> {
  return Promise.resolve(mockTextFiles.get(path) ?? null)
}

/** 写应用自有文本文件 */
export function mockWriteText(path: string, text: string): Promise<void> {
  mockTextFiles.set(path, text)
  return Promise.resolve()
}
