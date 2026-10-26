# PRD: git worktree 后端能力

父任务：`10-26-task-worktree`。设计：`../10-26-task-worktree/design.md` §2。
参考：`../10-26-task-worktree/research/codeg-worktree.md` §2、§7。

## Goal

给这个完全没有 git 能力的项目，补一层**最小但安全**的 worktree 操作，并让任务能记住自己的
工作树与分支。

## Requirements

### R1 新增模块 `src-tauri/src/worktree.rs`

三个命令，全部 camelCase、`Result<T, String>`：

| 命令 | 入参 | 出参 |
|---|---|---|
| `worktree_repo_info` | `rootPath` | `RepoInfo { isRepo, currentBranch, branches, detail }` |
| `worktree_create` | `CreateWorktreeRequest` | `CreatedWorktree { worktreePath, workBranch, baseSha, baseBranch, initError? }` |
| `worktree_remove` | `rootPath, worktreePath, workBranch, baseBranch?, baseSha?` | `()` |

### R2 创建流程（严格顺序）

1. 解析 `base_branch`（请求为空则用当前分支）
2. **解析 `base_sha`（必须在创建之前）**
3. `work_branch = "task/<taskId>"`
4. 计算绝对路径 `<项目父目录>/<项目目录名>-task-<taskId>`
5. 路径已存在 → 报错
6. `git worktree prune`
7. `git worktree add <abs> <work_branch>`
8. `git -C <abs> symbolic-ref --quiet HEAD` == `refs/heads/<work_branch>`，
   否则 `git worktree remove --force` 回滚并报错
9. `initCommand` 非空则执行一次，失败只记 `initError`，不回滚
10. 返回

### R3 路径安全

- 交给 git 的 worktree 路径必须是**绝对路径**
- 盘符相对路径（`C:foo` 形式）与非 UTF-8 路径**直接拒绝**

### R4 删除流程

1. 拒绝非绝对路径 / 盘符相对路径
2. `git worktree remove --force <abs>`；目录已不存在视为成功
3. `git worktree prune`
4. 分支有未落地提交（tip 既不是 base 祖先、tree 也不相同）→ **保留分支**
5. 否则 `git branch -D <work_branch>`

### R5 任务字段

`WorkTask` 新增（`#[serde(default)]`，旧数据可读）：
`use_worktree: bool`、`worktree_path`、`work_branch`、`base_branch`、`base_sha`、`init_command`。

`TaskDraft` / `TaskPatch` 只加 `use_worktree` / `base_branch` / `init_command`
（路径、分支名、sha 由创建流程写入，不接受客户端传入）。

新命令：`task_attach_worktree`、`task_clear_worktree`。

### R6 前端契约

`types/task.ts`、`taskService.ts`、`taskMock.ts`、`stores/task.ts` 同步；
新增 `src/services/worktreeService.ts`（`isTauri()` 双通道，浏览器 mock 返可用的假数据）。

## Acceptance Criteria

- [ ] **AC1** 非 git 目录调 `worktree_repo_info` → `isRepo:false` 且 `detail` 说明原因，不抛错。
- [ ] **AC2** detached HEAD 时 `currentBranch` 为 `null`。
- [ ] **AC3** `worktree_create` 返回的 `baseSha` 等于调用**前** `git rev-parse <base_branch>` 的结果。
- [ ] **AC4** 创建成功后 `git worktree list` 含该路径，且在该目录 `git symbolic-ref HEAD` 返回 `refs/heads/task/<id>`。
- [ ] **AC5** 目标路径已存在时返回明确错误，不覆盖。
- [ ] **AC6** 传入盘符相对路径（如 `C:foo`）被拒绝，且**不执行任何 git 命令**。
- [ ] **AC7** `worktree_remove` 后目录消失、`git worktree list` 中该条目消失。
- [ ] **AC8** 分支有未落地提交时，`worktree_remove` 成功但分支仍在。
- [ ] **AC9** 旧 `tasks.json`（无新字段）能正常读取，新字段取默认值。
- [ ] **AC10** `WorkTask` 序列化含 `useWorktree` / `worktreePath` / `workBranch` / `baseBranch` / `baseSha` / `initCommand`。
- [ ] **AC11** `cargo check` + `cargo test --lib` + `pnpm vue-tsc --noEmit` + `pnpm build` 全绿。

## Out of Scope

- 编辑器 UI（子任务 2）
- 启动链路与清理触发（子任务 3）
- 远端分支、合并、preflight
- `worktree_root` 自定义目录
