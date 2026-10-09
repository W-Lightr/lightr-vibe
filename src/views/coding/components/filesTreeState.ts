// 文件树懒加载控制器：FilesTree（根）与 FilesTreeNode（递归子组件）共享的
// 展开/缓存/loading/错误状态与动作（design D2：展开时请求 + 缓存）。
// 独立模块避免 SFC 循环导入；组件本身仍各自为 .vue。

import { ref, type InjectionKey, type Ref } from 'vue'
import type { DirEntry } from '../../../types/project'
import { fsErrorMessage } from '../../../types/project'
import { fsService, joinPath } from '../../../services/fsService'

export interface TreeController {
  /** 已加载目录的条目缓存（absPath -> entries；展开过不重复请求） */
  cache: Ref<Map<string, DirEntry[]>>
  expanded: Ref<Set<string>>
  loading: Ref<Set<string>>
  errors: Ref<Map<string, string>>
  entriesOf(path: string): DirEntry[] | undefined
  isExpanded(path: string): boolean
  isLoading(path: string): boolean
  errorOf(path: string): string | null
  toggle(path: string): void
  retry(path: string): void
  loadDir(path: string): Promise<void>
  openFile(absPath: string, name: string): void
  /** 项目切换时清空全部状态 */
  reset(): void
  /** 手动刷新：清缓存后重载根与已展开目录 */
  refreshAll(): Promise<void>
  joinPath(dir: string, name: string): string
}

export const treeControllerKey: InjectionKey<TreeController> = Symbol('files-tree-controller')

/**
 * 创建控制器实例。
 * @param getRoot 当前项目根路径（null = 无关联项目）
 * @param onOpenFile 双击文件的打开动作（由组件接 workspace.openFile）
 */
export function createTreeController(
  getRoot: () => string | null,
  onOpenFile: (absPath: string, name: string) => void,
): TreeController {
  const cache = ref<Map<string, DirEntry[]>>(new Map())
  const expanded = ref<Set<string>>(new Set())
  const loading = ref<Set<string>>(new Set())
  const errors = ref<Map<string, string>>(new Map())

  async function loadDir(path: string): Promise<void> {
    const root = getRoot()
    if (!root || cache.value.has(path) || loading.value.has(path)) return
    loading.value.add(path)
    errors.value.delete(path)
    try {
      const entries = await fsService.listDir(path, root)
      cache.value.set(path, entries)
    } catch (err) {
      errors.value.set(path, fsErrorMessage(err))
    } finally {
      loading.value.delete(path)
    }
  }

  function toggle(path: string): void {
    if (expanded.value.has(path)) {
      expanded.value.delete(path)
    } else {
      expanded.value.add(path)
      void loadDir(path)
    }
  }

  /** 错误行内重试：清该目录的缓存与错误后重新加载 */
  function retry(path: string): void {
    cache.value.delete(path)
    errors.value.delete(path)
    void loadDir(path)
  }

  function reset(): void {
    cache.value = new Map()
    expanded.value = new Set()
    loading.value = new Set()
    errors.value = new Map()
  }

  async function refreshAll(): Promise<void> {
    const root = getRoot()
    const expandedPaths = [...expanded.value]
    reset()
    if (!root) return
    expanded.value = new Set(expandedPaths)
    await loadDir(root)
    await Promise.all(expandedPaths.map((p) => loadDir(p)))
  }

  return {
    cache,
    expanded,
    loading,
    errors,
    entriesOf: (path) => cache.value.get(path),
    isExpanded: (path) => expanded.value.has(path),
    isLoading: (path) => loading.value.has(path),
    errorOf: (path) => errors.value.get(path) ?? null,
    toggle,
    retry,
    loadDir,
    openFile: onOpenFile,
    reset,
    refreshAll,
    joinPath,
  }
}
