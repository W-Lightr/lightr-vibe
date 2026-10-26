# 删除 worktree 前自动提交未保存改动

## 背景

上一轮修好了「任务完成后保留分支」，但留下一个**真实的数据丢失口子**：

Agent 在 worktree 里改了文件却**没有 commit**，清理时 `git worktree remove --force` 会连工作区
改动一起删掉。分支虽然保留，但分支上没有任何这次改动的痕迹 —— 成果仍然丢了。

用户实测过这个场景：`git branch --list "task/*"` 空、`git fsck --unreachable` 也无输出，
说明改动从未进入 git 对象库。

## Goal

**删除 worktree 目录之前，先把工作区里的未提交改动自动提交到 `task/<id>` 分支。**
这样即使 Agent 忘了 commit，成果也能通过分支找回。

## Requirements

### R1 新增命令 `worktree_commit_all`

- 入参：`worktreePath`、`message`
- 在该 worktree 内执行：`git add -A` 然后 `git commit -m <message>`
- **没有改动时不产生空提交**：先探测是否干净（`git status --porcelain` 为空），干净则直接返回
  `committed: false`
- 返回 `{ committed: boolean, sha?: string }`
- `message` 由调用方给（建议用任务标题）

### R2 终态清理前调用

`cleanupWorktreeOnFinish` 的顺序改为：

```
1. worktree_commit_all(worktreePath, 任务标题)
2. worktree_remove(..., keepBranch: true)
```

- 提交失败**不阻断**清理：目录照删、分支照留，但把失败原因写进 `lastError`
  并在详情里提示「自动提交失败，未提交的改动已丢失」
- 已经是干净工作区 → 静默跳过（无 toast、无 `lastError`）

### R3 不影响回滚路径

启动失败回滚（`failSetup` / `rollbackStart`）**不调用** `worktree_commit_all`——
那次尝试没有价值，直接删目录 + 删分支。

### R4 详情提示

任务终态且分支保留时，详情的工作树区显示：

- 自动提交成功 → 「已自动提交未保存改动到 `task/xxx`」
- 自动提交失败 → 「自动提交失败：<原因>（未提交的改动已丢失）」
- 无改动 → 不显示额外提示

## Acceptance Criteria

- [ ] **AC1** Agent 改了文件但没 commit，任务完成后 `git log task/xxx` 能看到一个自动提交。
- [ ] **AC2** 自动提交的 message 包含任务标题。
- [ ] **AC3** 工作区本来干净时，不产生空提交（`git log` 条数不变）。
- [ ] **AC4** 自动提交失败（如 git 身份未配置）时，目录仍被删除、分支仍保留，`lastError` 有原因。
- [ ] **AC5** 启动失败回滚路径**不**产生自动提交（分支应被删除，`git branch` 无残留）。
- [ ] **AC6** `worktree_commit_all` 同样拒绝非绝对路径 / 盘符相对路径，且不执行任何 git 命令就报错。
- [ ] **AC7** Rust 单测：有改动 → 提交成功且 sha 非空；无改动 → `committed: false` 且无新提交。
- [ ] **AC8** `cargo check` + `cargo test --lib` + `pnpm vue-tsc --noEmit` + `pnpm build` 全绿。

## 约束

- 自动提交只能落在 **worktree 内的 task 分支**上，绝不能碰项目主分支。
- 不使用 `--force`、不改 git 全局配置（缺 user.name/email 时就是失败，R4 提示用户自行配置）。
- 不引入新依赖。

## Out of Scope

- 自动合并分支回主分支
- 自动解决冲突
- 提交历史整理 / squash
- 自动提交失败后的重试 UI
