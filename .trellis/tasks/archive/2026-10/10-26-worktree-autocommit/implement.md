# Implement: 删除 worktree 前自动提交未保存改动

需求 `prd.md`，设计 `design.md`。

## 验证命令

```powershell
cargo check                       # workdir: src-tauri
cargo test --lib                  # workdir: src-tauri
pnpm vue-tsc --noEmit
pnpm build
```

---

## Step 1 · Rust：`worktree_commit_all`

- [ ] `src-tauri/src/worktree.rs`
  - [ ] `CommitWorktreeRequest` / `CommitWorktreeResult`（camelCase）
  - [ ] `commit_inner(&req) -> Result<CommitWorktreeResult, String>`
    - [ ] `require_absolute` 安全检查（与 remove 同一套）
    - [ ] 目录不存在 → `Err("worktree-missing")`
    - [ ] `git status --porcelain` 空 → `Ok({ committed: false, sha: None })`
    - [ ] `git add -A` → `git commit -m <message>` → `git rev-parse HEAD`
    - [ ] 每步失败带 stderr 尾部，前缀 `git <子命令>:`
  - [ ] `#[tauri::command] pub fn worktree_commit_all(req)` 注册到 `lib.rs`
- [ ] 三个真实 git 测试
  - [ ] `commit_all_commits_dirty_worktree`
  - [ ] `commit_all_skips_a_clean_worktree`
  - [ ] `commit_all_rejects_relative_path`

**验证**：`cargo test --lib`
**回滚点**：`git checkout src-tauri/src/worktree.rs src-tauri/src/lib.rs`

## Step 2 · 前端 service

- [ ] `src/services/worktreeService.ts`：`commitAll({ worktreePath, message })`
- [ ] mock 返回 `{ committed: false, sha: null }`

**验证**：`pnpm vue-tsc --noEmit`

## Step 3 · 清理链路

- [ ] `src/views/tasks/taskSessionBridge.ts`
  - [ ] 新增 `commitWorktreeIfDirty(row)`：无 worktreePath 跳过；失败返回可读串不抛
  - [ ] `cleanupWorktreeOnFinish`：**先** `commitWorktreeIfDirty`，**再** `discardWorktree(..., true)`
  - [ ] 提交失败时把原因写进 `lastError`（不阻断删除与保留分支）
  - [ ] 确认 `failSetup` / `rollbackStart` **不**调用自动提交
- [ ] `src/views/tasks/components/setupErrorPrefix.ts` 加 `WORKTREE_AUTOCOMMIT_FAILED_PREFIX`

**验证**：`pnpm vue-tsc --noEmit` + `pnpm build`
**回滚点**：`git checkout src/views/tasks/taskSessionBridge.ts src/views/tasks/components/setupErrorPrefix.ts`

## Step 4 · 详情提示

- [ ] `src/views/tasks/components/TaskDetailDialog.vue` 工作树区
  - [ ] 自动提交失败 → 琥珀色提示 + 原因
  - [ ] 终态且分支保留且无失败 → 灰色「已自动提交未保存改动到 task/xxx」
  - [ ] 无改动 → 不显示

**验证**：`pnpm vue-tsc --noEmit` + `pnpm build`
**回滚点**：`git checkout src/views/tasks/components/TaskDetailDialog.vue`

## Step 5 · 全量检查

- [ ] 四门禁全绿
- [ ] 读代码确认 `commitWorktreeIfDirty` 在 `discardWorktree` **之前**（顺序不能反）

---

## 关键契约

- **先 commit 再 remove**：反了目录已消失，`git add` 无从谈起。
- **提交失败不阻断清理**：目录照删、分支照留，`lastError` 说明原因。
- **只剩回滚路径不提交**：那次尝试没有价值。
- 不 `--no-verify`、不 `git config`、不 push、不 `--amend`。
- 提交 message 带可辨识前缀，避免被误认为是用户自己的提交。

## 人工验收

- [ ] 勾 worktree 跑任务，让 Agent 改文件但**不** commit
- [ ] 点「通过」→ `git log task/xxx` 能看到自动提交，`git show` 有那些改动
- [ ] 再跑一个任务，Agent 自己 commit 了 → 无额外空提交
- [ ] 故意清空 git 身份（`git config user.name ''`）跑任务 → 目录删除、分支保留、详情显示提交失败
- [ ] 启动失败的任务 → `git branch` 无残留、无自动提交
