// git worktree 服务层：worktree_* 命令的唯一入口（tauri-bridge spec §2 边界）。
// isTauri() 双通道：Tauri 走 Rust invoke（git 调用只发生在 src-tauri/src/worktree.rs，
// PAC2）；浏览器 dev 走本文件的 mock，返回**可用**的假数据，便于不装 git 也能调 UI。
// 参数一律 camelCase，与 Rust #[serde(rename_all = "camelCase")] 对齐。
//
// 错误串（Rust 侧）：worktree-not-repo / path-not-absolute / path-not-utf8 /
// worktree-path-exists / worktree-add: … / worktree-detached / worktree-base-branch: …

import { invoke } from '@tauri-apps/api/core'
import type {
  CreateWorktreeRequest,
  CreatedWorktree,
  RepoInfo,
} from '../types/task'
import { isTauri } from '../utils/isTauri'

/** 浏览器 dev 的假仓库信息（开关可打开、分支可选，方便调试编辑器） */
function mockRepoInfo(): RepoInfo {
  return { isRepo: true, currentBranch: 'main', branches: ['main'], detail: null }
}

/**
 * 浏览器 dev 的假创建结果：路径与分支按 Rust 的命名规则拼出来
 * （`<项目父目录>/<项目目录名>-task-<taskId>`、`task/<taskId>`）。
 * sha 用固定占位值——浏览器里没有 git，UI 只做展示。
 */
function mockCreate(req: CreateWorktreeRequest): CreatedWorktree {
  const normalized = req.rootPath.replace(/\\/g, '/').replace(/\/+$/, '')
  const parent = normalized.slice(0, normalized.lastIndexOf('/'))
  const name = normalized.slice(normalized.lastIndexOf('/') + 1)
  return {
    worktreePath: `${parent}/${name}-task-${req.taskId}`,
    workBranch: `task/${req.taskId}`,
    baseSha: 'mock0000000000000000000000000000000000000',
    baseBranch: req.baseBranch?.trim() || 'main',
    // mock 里 init 命令永远「成功」，避免 dev 环境无故飘红
    initError: null,
  }
}

export const worktreeService = {
  /**
   * 仓库信息：是否 git 仓库 + 当前分支 + 本地分支列表。**永不抛错**——
   * 失败（非仓库 / 没装 git）表现为 `isRepo: false` + `detail`。
   */
  async repoInfo(rootPath: string): Promise<RepoInfo> {
    if (!isTauri()) return mockRepoInfo()
    return invoke<RepoInfo>('worktree_repo_info', { rootPath })
  },

  /** 创建工作树；`initError` 非空表示目录已就绪但初始化命令失败（不回滚） */
  async create(req: CreateWorktreeRequest): Promise<CreatedWorktree> {
    if (!isTauri()) return mockCreate(req)
    return invoke<CreatedWorktree>('worktree_create', { req })
  },

  /**
   * 删除工作树 +（分支无未落地提交时）删除任务分支。
   * 目录已不存在视为成功；分支有未落地提交时**保留分支**（同样返回成功）。
   */
  async remove(
    rootPath: string,
    worktreePath: string,
    workBranch: string,
    baseBranch: string | null,
    baseSha: string | null,
  ): Promise<void> {
    if (!isTauri()) return
    await invoke('worktree_remove', {
      rootPath,
      worktreePath,
      workBranch,
      baseBranch,
      baseSha,
    })
  },
}
