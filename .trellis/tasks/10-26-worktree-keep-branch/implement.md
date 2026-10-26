# Implement: worktree 完成后保留分支

需求 `prd.md`，设计 `design.md`。

## 验证命令

```powershell
cargo check                       # workdir: src-tauri
cargo test --lib                  # workdir: src-tauri
pnpm vue-tsc --noEmit
pnpm build
```

---

## Step 1 · Rust：`worktree_remove` 加 `keepBranch`

- [x] `src-tauri/src/worktree.rs`
  - [x] `remove_inner(..., keep_branch: bool)` 新增参数
  - [x] 在**分支守卫之前**插入 `if keep_branch { return Ok(()) }`
    （位置必须是 prune 之后、`branch_holds_unlanded_work` 之前）
  - [x] `worktree_remove` 命令签名加 `#[serde(default)] keep_branch: bool`
        —— **实现与设计不同**：Tauri 对**函数参数**只允许内置属性，`#[serde(default)]`
        写在参数上是编译错误。改为入参结构体 `RemoveWorktreeRequest`
        （`#[derive(Deserialize)] #[serde(rename_all = "camelCase")]` + 字段级
        `#[serde(default)]`），命令变成 `worktree_remove(req)`；前端 payload 相应
        包一层 `{ req: { ... } }`（与既有 `worktree_create(req)` 同款）。
- [x] 补两个真实 git 测试
  - [x] `keep_branch_leaves_the_branch`：目录消失、分支仍在（并断言分支上的提交还在）
  - [x] `default_remove_still_deletes_the_branch`：两者都消失
- [x] 检查期追加 `empty_worktree_path_deletes_only_the_branch`：
      目录已删 → 前端传空 `worktreePath` 时必须跳过目录删除、真的删掉分支
      （`remove_inner_allow_absent_dir`；否则 AC5 永远是 `path-not-absolute`）

**验证**：`cargo test --lib` → 94 passed（原 91）
**回滚点**：`git checkout src-tauri/src/worktree.rs`

## Step 2 · 前端 service

- [x] `src/services/worktreeService.ts`：`remove` 参数加 `keepBranch = false`（可选）
- [x] mock 分支同步：`!isTauri()` 时直接 return，签名已接受该参数

**验证**：`pnpm vue-tsc --noEmit`

## Step 3 · 清理链路

- [x] `src/views/tasks/taskSessionBridge.ts`
  - [x] `discardWorktree(taskId, known, keepBranch = false)` 增加第三参数
  - [x] `cleanupWorktreeOnFinish` 调用时传 `true`（`:397`）
  - [x] `failSetup`（`:356`）/ `rollbackStart`（`:79`）/ stale 清理（`:237`）/
        `retryWorktreeCleanup`（`:429`）保持默认 `false`
- [x] 确认终态三条路径（approveTask `:503` / stopTask `:490` / transitionTask `:416`）
      都经 `cleanupWorktreeOnFinish` 走 `true`；`rejectTask`（review→todo）不经过它

**验证**：`pnpm vue-tsc --noEmit` + `pnpm build`
**回滚点**：`git checkout src/views/tasks/taskSessionBridge.ts`

## Step 4 · 详情抽屉

- [x] `src/views/tasks/components/TaskDetailDialog.vue`
  - [x] 「工作树」区：有分支、任务已结束（**done / canceled**）、目录已删时
        显示蓝色「分支 task/xxx 已保留，可用 git log / git diff / git merge」
        ——检查期收紧：`review` / `failed` 时目录通常还在，那时走「重试清理」，
        不能显示「目录已删」的错误文案、也不能跳过清理直接删分支
  - [x] 「删除分支」按钮 + `NPopconfirm` 二次确认（写明明未合并提交会变不可达、不可撤销）
  - [x] 删除成功（`deleteWorktreeBranch`）后 `taskStore.clearWorktree`
  - [x] 文案说明「工作树目录已删除：Agent 未提交的工作区改动已随之丢失」

**验证**：`pnpm vue-tsc --noEmit` + `pnpm build`
**回滚点**：`git checkout src/views/tasks/components/TaskDetailDialog.vue`

## Step 5 · 全量检查

- [x] `cargo check` + `cargo test --lib` + `pnpm vue-tsc --noEmit` + `pnpm build` 全绿
- [x] 确认 `keep_branch` 短路位置在守卫之前（`worktree.rs:543` vs `:564`）

---

## 关键契约

- **`keepBranch` 必须短路在分支守卫之前**——放在后面等于没改。
- **不动** `worktree_remove` 的默认行为（AC4）：`keepBranch` 省略 = false。
- 未提交改动的丢失是 worktree 语义问题，本次只做文案提示，不做自动 commit。

## 人工验收

- [ ] 勾 worktree 跑一个任务，让 Agent 改点东西并提交
- [ ] 点「通过」→ 目录消失，`git branch --list "task/*"` **有输出**
- [ ] `git log task/xxx` 能看到 Agent 的提交
- [ ] 详情里点「删除分支」→ 确认 → 分支消失
- [ ] 另一个任务故意让启动失败（如选冲突分支）→ 确认分支**没有**残留
