# worktree 完成后保留分支

## 背景

用户实测：勾选 worktree 跑完任务后，worktree 目录被删了，`git branch --list "task/*"` 为空，
`git fsck --unreachable` 也找不到孤立提交 —— **Agent 的成果完全消失**。

## 根因

`src-tauri/src/worktree.rs::remove_inner` 在删掉 worktree 目录之后，还有两步：

```
4. 分支守卫：分支 tip 若无未落地提交才继续
5. git branch -D <work_branch>
```

这两步对**所有**调用方一视同仁。但调用方有两种完全不同的语义：

| 调用场景 | 期望 |
|---|---|
| **任务终态清理**（done / canceled） | 删目录，**保留分支** —— 那是任务的产品 |
| **启动失败回滚**（created 但后续步骤失败） | 删目录 + 删分支 —— 那次尝试没有价值 |

现在两种场景都走「能删就删」，于是任务做完分支也没了。

## Goal

- 任务**完成后**，worktree 目录删除，**分支必须保留**，可用 `git log` / `git diff` / `git merge`。
- 任务**取消后**，同上（用户可能还要看改了什么）。
- 启动失败回滚，仍删除目录与分支（清理无效尝试）。
- 用户能手动删除已保留的分支。

## Requirements

### R1 `worktree_remove` 增加 `keepBranch` 参数

- 签名变为 `worktree_remove { rootPath, worktreePath, workBranch, baseBranch?, baseSha?, keepBranch? }`
- `keepBranch` 省略 / false → 保持现有行为（删目录 + 尽力删分支）
- `keepBranch: true` → **只删目录与注册**，完全不碰分支

### R2 终态清理传 `keepBranch: true`

- 任务进入 `done` 或 `canceled` 的清理路径传 `true`
- 启动失败回滚路径保持默认（删分支）

### R3 详情抽屉提供手动删除分支

- 有 worktree 痕迹（`workBranch != null`）时显示「删除分支」按钮
- 点击 → 调 `worktree_remove(keepBranch: false)` 或新的专用命令
- 需要二次确认（不可逆）

### R4 提示「改动仍未提交时会丢」

- worktree 删除时，若 Agent 只改了工作区没 commit，`--force` 会连改动一起删
- 详情里对已完成但有分支的任务，提示「分支 task/xxx 已保留，可 git log 查看」

## Acceptance Criteria

- [x] **AC1** 任务完成后：worktree 目录消失、`git worktree list` 恢复，但 `git branch --list "task/*"` **仍有该分支**。（代码层：`cleanupWorktreeOnFinish` → `discardWorktree(..., true)`；Rust 测试 `keep_branch_leaves_the_branch`）
- [x] **AC2** 任务取消后：同上（分支保留）。（`stopTask` / `transitionTask` 同样经 `cleanupWorktreeOnFinish`）
- [x] **AC3** 启动失败回滚（worktree 已建但 attachSession 失败）：目录与分支**都被删除**（行为不变）。（`failSetup` / `rollbackStart` 保持默认 `keepBranch=false`）
- [x] **AC4** `worktree_remove` 不传 `keepBranch` 时行为与改动前一致。（`#[serde(default)]` + TS 默认 `false`；旧调用零改动）
- [x] **AC5** 详情抽屉能手动删除已保留的分支，删除后 `git branch` 不再有它。（`deleteWorktreeBranch` → `worktree_remove(keepBranch=false)` → `clearWorktree`）
- [x] **AC6** 分支仍有未合并提交时，手动删除也要二次确认（防误删）。（`NPopconfirm` 对**所有**手动删除都要求确认，文案写明未合并提交会变不可达）
- [x] **AC7** Rust 单测：`keep_branch` 场景下目录消失、分支仍在；非 `keep_branch` 场景两者都消失。（`keep_branch_leaves_the_branch` / `default_remove_still_deletes_the_branch` / 追加 `empty_worktree_path_deletes_only_the_branch`）
- [x] **AC8** `cargo check` + `cargo test --lib` + `pnpm vue-tsc --noEmit` + `pnpm build` 全绿。（94 tests passed）

## 额外说明（不在本次修复范围）

即使保留分支，**Agent 没有 commit 的改动仍然会随目录删除而丢失** —— 这是 worktree 语义决定的，
不是本次能解决的问题。长期方案是要求 Agent 结束前提交，或在删除前把工作区改动 stash 到分支。
本次不实现，但要在详情文案里说明清楚。

## Out of Scope

- 自动 commit Agent 的工作区改动
- 自动合并分支回主分支
- diff 审核视图
