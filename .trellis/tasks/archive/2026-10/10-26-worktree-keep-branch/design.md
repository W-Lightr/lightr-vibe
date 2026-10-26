# Design: worktree 完成后保留分支

需求 `prd.md`。

## 1. 问题定位

`src-tauri/src/worktree.rs::remove_inner`（`:513-569`）：

```rust
// 2) 删目录
git worktree remove --force <abs>
// 3) prune
git worktree prune
// 4) 分支守卫
if branch_holds_unlanded_work(root, work_branch, &base) { return Ok(()); }
// 5) 删分支
git branch -D <work_branch>
```

调用方（`src/views/tasks/taskSessionBridge.ts`）：

| 位置 | 场景 | 传给 git 的语义 |
|---|---|---|
| `:350` `discardWorktree(taskId, created)` in `failSetup` | 启动失败回滚 | 应删分支 |
| `:406` `cleanupWorktreeOnFinish` via `approveTask` | review → **done** | **应保留分支** |
| `:450` `cleanupWorktreeOnFinish` via `stopTask` | → **canceled** | **应保留分支** |
| `:463` `cleanupWorktreeOnFinish` via `transitionTask` | 拖拽到终态 | **应保留分支** |

第 4 步的守卫只拦「有未落地提交」的情况；任务正常完成时若 Agent 的提交已被合并（或
tip 是 base 祖先），守卫放行 → 分支被删。

## 2. Rust 改动

### 2.1 签名

```rust
#[tauri::command]
pub fn worktree_remove(
    root_path: String,
    worktree_path: String,
    work_branch: String,
    base_branch: Option<String>,
    base_sha: Option<String>,
    /// 只删工作树、保留分支（任务终态用）。省略 = false（失败回滚用）
    #[serde(default)]
    keep_branch: bool,
) -> Result<(), String>
```

`remove_inner` 同步加参数。

### 2.2 内部逻辑

```
1. require_absolute(root) / require_absolute(worktree_path)   // 不变
2. git worktree remove --force <abs>                          // 不变
3. git worktree prune                                         // 不变
4. if keep_branch { return Ok(()) }   ← 新增，在分支守卫之前短路
5~6. 分支守卫 + branch -D                                      // 不变
```

**关键**：`keep_branch` 必须在第 4 步**之前**短路，否则仍会走守卫与删除。

### 2.3 新增测试

| 测试 | 断言 |
|---|---|
| `keep_branch_leaves_the_branch` | 真实 git repo：create → remove(keep_branch=true) → 目录不存在、`git branch --list task/x` 有输出 |
| `default_remove_still_deletes_the_branch` | create → remove(默认) → 目录与分支都消失 |

## 3. 前端改动

### 3.1 `worktreeService.ts`

```ts
remove(params: {
  rootPath: string
  worktreePath: string
  workBranch: string
  baseBranch?: string | null
  baseSha?: string | null
  /** 只删目录、保留分支（任务终态用） */
  keepBranch?: boolean
}): Promise<void>
```

### 3.2 `taskSessionBridge.ts`

`discardWorktree(taskId, created, keepBranch)` 增加第三个参数：

- `cleanupWorktreeOnFinish` → `keepBranch: true`
- `failSetup` 的回滚 → `keepBranch: false`（默认）
- `rollbackStart` 里的回滚 → `false`

### 3.3 详情抽屉

`TaskDetailDialog.vue` 的「工作树」区：

- 当 `task.workBranch != null` 时显示一行：「分支 `task/xxx` 已保留，可用 `git log` 查看」
- 右侧「删除分支」按钮 → 二次确认（`NPopconfirm` 或原生 confirm）→
  调 `worktreeService.remove({ ..., keepBranch: false })` → 成功后 `clearWorktree`
- 目录已删（`worktreePath == null`）但分支还在时，这个按钮仍然可用：
  `worktree_remove` 对不存在的目录是宽容的（第 2 步「目录已不存在视为成功」）

### 3.4 文案

删目录后若 `worktreePath` 被清空但分支保留，详情显示：

> 分支 `task/xxx` 已保留。若 Agent 有未提交的改动，它们随工作树删除而丢失。

## 4. 兼容 / 回滚

- `keepBranch` 是可选参数（`#[serde(default)]` + TS `?:`），旧调用不受影响
- 回滚 = `git checkout` 四个文件（`worktree.rs` / `worktreeService.ts` /
  `taskSessionBridge.ts` / `TaskDetailDialog.vue`）

## 5. 风险

| 风险 | 缓解 |
|---|---|
| 分支越积越多 | 详情提供「删除分支」；后续可加批量清理 |
| 用户以为「保留分支」= 改动一定在 | 文案明确说明未提交改动会丢（PRD 额外说明） |
| 手动删分支误删未合并提交 | 二次确认 + 文案写明不可逆 |
| `keep_branch` 加错位置（守卫之后） | 测试 `keep_branch_leaves_the_branch` 锁死行为 |
