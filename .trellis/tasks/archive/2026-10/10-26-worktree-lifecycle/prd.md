# PRD: worktree 启动链路与自动清理

父任务：`10-26-task-worktree`。前置：`10-26-worktree-backend`、`10-26-worktree-editor`。
设计：`../10-26-task-worktree/design.md` §3.4、§3.5。

## Goal

勾了 worktree 的任务真的在独立工作树里跑；不勾的行为一点都不变。任务结束，工作树自动消失。

## Requirements

### R1 启动链路分支

`startTask` 里，在建会话之前决定 cwd：

```
useWorktree === false  → cwd = project.rootPath（与现状完全一致）
useWorktree === true   → worktreeService.create(...)
                          → taskStore.attachWorktree(id, info)
                          → cwd = info.worktreePath
```

### R2 失败处理

- `worktree_create` 失败 → 任务 `failed`，`failureReason = 'setup_error'`，
  **不创建会话**，错误原样提示。
- worktree 已创建、但后续步骤（attach session / preparing / running）失败 →
  先 `worktreeService.remove` 再回退任务状态，不留孤儿目录。
- `initCommand` 失败**不阻断**启动（`initError` 写进 `lastError` 供参考）。

### R3 终态清理

- 任务进入 `done` 或 `canceled`，且 `worktreePath != null` →
  `worktreeService.remove(...)` → 成功后 `taskStore.clearWorktree(id)`。
- 删除失败：
  - 写 `lastError`（含路径）
  - **不回滚任务状态**
  - 详情抽屉显示「工作树清理失败」+ 路径 + 重试按钮

### R4 详情展示

- 有 worktree 的任务，详情显示：路径、工作分支、基线分支与 sha。
- 提供「复制路径」按钮。

## Acceptance Criteria

- [x] **AC1** 不勾 worktree：启动流程与改动前逐字节一致，无任何 git 调用。
  （代码层：`cwd` 初值即 `project.rootPath`，只在 `if (queued.useWorktree)` 分支内被改写；
  该分支是本链路唯一触摸 `worktreeService` 的地方。真机验收待人工。）
- [x] **AC2** 勾了 worktree：启动后 `git worktree list` 含该路径。（代码层，真机待验收）
- [x] **AC3** 会话 cwd 在 worktree 内（Agent 执行 `pwd` 验证）。（`createDraft({ cwd })` 传入 worktreePath）
- [x] **AC4** 项目根目录 `git status` 在任务全程无新增改动。（worktree 从提交拉出，不触碰工作区）
- [x] **AC5** 任务完成 → worktree 目录消失、`git worktree list` 恢复。（`approveTask`）
- [x] **AC6** 任务取消 → 同上。（`stopTask` + 看板拖拽 `transitionTask`）
- [x] **AC7** `worktree_create` 失败 → 任务 `failed(setup_error)`，无会话被创建。（
  `failSetup`：`lastError` 以「准备失败：」开头，chip 显示「失败 · 启动失败」）
- [x] **AC8** worktree 已建但后续步骤失败 → 目录被清理，任务状态正确回退。（`rollbackStart(..., createdWorktree)`）
- [x] **AC9** 删除失败 → 任务状态不回滚，详情有「清理失败」提示与路径。（+「重试清理」按钮）
- [x] **AC10** `initCommand` 失败不阻断任务启动。（`initError` 只写 `lastError`）
- [x] **AC11** `cargo check` + `cargo test --lib` + `pnpm vue-tsc --noEmit` + `pnpm build` 全绿。

## Out of Scope

- 合并、diff 审核、PR
- 磁盘占用统计与配额
- 并发队列
