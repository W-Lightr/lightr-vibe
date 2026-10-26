# Design: 删除 worktree 前自动提交未保存改动

需求 `prd.md`。

## 1. 为什么需要这一步

`git worktree remove --force` 删除的是**工作目录**。目录里的未提交改动不在 git 对象库中，
删掉即永久丢失 —— 任何「保留分支」的补救都无能为力，因为分支上根本没有那些内容。

所以唯一能保住它们的时机是**删除之前**把它们变成一次 commit。

## 2. Rust：`src-tauri/src/worktree.rs`

### 2.1 命令

```rust
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitWorktreeRequest {
    pub worktree_path: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitWorktreeResult {
    /// 是否真的产生了提交（工作区本来就干净 → false）
    pub committed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha: Option<String>,
}

#[tauri::command]
pub fn worktree_commit_all(req: CommitWorktreeRequest) -> Result<CommitWorktreeResult, String>
```

> 注意：与 `worktree_remove` 一样用**入参结构体**，不用带 `#[serde(default)]` 的裸参数
> （Tauri 不允许在命令参数上写 serde 属性）。

### 2.2 实现步骤

```
1. require_absolute(worktree_path)                  ← 与 remove 同一套安全校验
2. 目录必须存在，否则 Err("worktree-missing")
3. git -C <abs> status --porcelain                  ← 探测是否有改动
     stderr/exit 非 0 → Err("git status: <tail>")
     输出为空 → return Ok({ committed: false })
4. git -C <abs> add -A
     失败 → Err("git add: <tail>")
5. git -C <abs> commit -m <message>
     失败 → Err("git commit: <tail>")             ← 典型是缺 user.name/email
6. git -C <abs> rev-parse HEAD                      → sha
7. return Ok({ committed: true, sha: Some(sha) })
```

**不做的事**：
- 不加 `--no-verify`（用户可能配了 hook，让 hook 生效是正确行为）
- 不 `git config` 任何东西（缺身份就是失败，交给用户）
- 不 `--amend`、不 push

### 2.3 测试（真实 git 仓库）

| 测试 | 断言 |
|---|---|
| `commit_all_commits_dirty_worktree` | 写一个文件 → commit_all → `committed: true`、sha 非空、`git log -1 --format=%s` 等于 message |
| `commit_all_skips_a_clean_worktree` | 干净 worktree → `committed: false`、`git rev-list --count HEAD` 不变 |
| `commit_all_rejects_relative_path` | `C:foo` / 相对路径 → `path-not-absolute`，且不改任何 git 状态 |

## 3. 前端

### 3.1 `src/services/worktreeService.ts`

```ts
commitAll(params: { worktreePath: string; message: string }): Promise<{ committed: boolean; sha: string | null }>
```

`isTauri()` 双通道；浏览器 mock 返回 `{ committed: false, sha: null }`。

### 3.2 `src/views/tasks/taskSessionBridge.ts` — `cleanupWorktreeOnFinish`

```ts
async function cleanupWorktreeOnFinish(row) {
  // 终态清理：先把未提交改动落成一次提交，再删目录（分支保留）
  const note = await commitWorktreeIfDirty(row)   // 新增
  const failure = await discardWorktree(row.id, row, true)
  // ...原有失败处理
}
```

`commitWorktreeIfDirty(row)`：

- `row.worktreePath == null` → 直接返回 null（没有工作树可提交）
- 调 `worktreeService.commitAll({ worktreePath, message: row.title })`
- 失败 → 返回可读失败串（由 `cleanupWorktreeOnFinish` 写进 `lastError`），**不抛出**
- 成功且 `committed === true` → 返回 null（无需提示）
- 成功且 `committed === false` → 返回 null（无改动，静默）

### 3.3 `TaskDetailDialog.vue`

工作树区新增一行状态（复用现有的 label/value 行样式）：

| 条件 | 文案 |
|---|---|
| `lastError` 含「自动提交失败」 | 琥珀色：`自动提交失败：<原因>（未提交的改动已丢失）` |
| 终态 + 分支保留 + 无失败 | 灰色：`已自动提交未保存改动到 task/xxx` |

判定用 `lastError` 前缀常量，与既有 `WORKTREE_CLEANUP_FAILED_PREFIX` 同款做法
（放在 `components/setupErrorPrefix.ts`）。

## 4. 顺序为什么不能反

**必须先 commit 再 remove。** 反过来的话目录已经没了，`git add` 无从谈起。

`cleanupWorktreeOnFinish` 里两行的顺序就是这条约束的落点，读代码时要能看到
`commitWorktreeIfDirty` 在 `discardWorktree` 之前。

## 5. 兼容 / 回滚

- 纯新增命令 + 一处顺序调整，不改变 `worktree_remove` 的语义
- 回滚 = `git checkout` 三个文件（`worktree.rs` / `worktreeService.ts` /
  `taskSessionBridge.ts`）+ 详情一行文案

## 6. 风险

| 风险 | 缓解 |
|---|---|
| 自动提交把垃圾文件也提了（`node_modules` 等） | `git add -A` 遵守 `.gitignore`；用户在 worktree 里装的依赖不会进提交（除非项目本身没忽略） |
| 缺 git 身份导致提交失败 | 不阻断清理；`lastError` 明确告知，用户配置后可手动补救 |
| 用户以为自己 commit 过了，结果多出一条自动提交 | message 带明确前缀，如 `chore(task): 自动提交未保存改动`，可辨识 |
| 提交后立刻删目录，用户没机会看 | 分支保留，`git log task/xxx` 可查（上一轮已实现） |
