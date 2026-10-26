# Research: codeg 的 git worktree 实现

参考仓库：`H:\AI\Light-vibe\codeg`（Tauri 2 + Rust，worktree 逻辑集中在 `src-tauri/src/work_task/`）。

本文件只记录**我们复刻所需的部分**。codeg 的合并 / PR 交付 / 冲突修复等不在此范围。

---

## 1. 数据模型

### 任务行（`src-tauri/src/db/entities/work_task.rs:49-139`）

| 列 | 类型 | 含义 |
|---|---|---|
| `worktree_folder_id` | `Option<i32>` | 指向 `folder` 表里那条 worktree 记录（软外键） |
| `base_branch` | `Option<String>` | worktree 从哪个分支拉出 |
| `base_sha` | `Option<String>` | **创建 worktree 之前**记录的提交，作为 diff 基准 |
| `work_branch` | `Option<String>` | 任务自己的分支名，形如 `task/<id>` |
| `cleanup_state` | `Option<String>` | `NULL` 正常；`'failed'` 表示清理失败可重试 |

`merge_state` / `pending_merge` 属于合并流程，本次不复刻。

### 文件夹设置（`src-tauri/src/models/work_task.rs:204-300`）

| 字段 | 默认 | 含义 |
|---|---|---|
| `worktree_root` | `None` | worktree 建在哪个目录下；空 = 项目旁边 |
| `init_command` | `None` | 新建 worktree 后、Agent 启动前跑一次的命令（**复用 worktree 时不重跑**） |
| `delete_worktree_default` | `true` | 合并对话框里「删 worktree」的默认勾选 |

### 迁移

`m20260801_000001_work_task.rs` 建表时即含 `worktree_folder_id` / `base_branch` / `base_sha` / `work_branch` / `cleanup_state`。

---

## 2. Git 命令层（`src-tauri/src/work_task/git.rs`，71 KB）

### 2.1 创建：`worktree_add_existing_branch`（`:482-521`）

```rust
// 1) 先 prune，清掉失效的注册
git worktree prune

// 2) 添加（注意：不是 -b，是复用已存在的分支）
git worktree add <worktree_path> <branch>

// 3) 关键校验：确认 HEAD 真的挂在该分支上
git -C <worktree_path> symbolic-ref --quiet HEAD
// 期望输出 == "refs/heads/<branch>"
```

**为什么需要第 3 步**（源码注释原文）：

> `worktree add <path> <name>` falls back to a **DETACHED** checkout when `<name>` is not a local branch (a same-name tag, say). A detached task worktree would collect commits nothing points to while the merge targets the wrong ref.

若校验失败 → `git worktree remove --force <path>` 回滚，返回错误。

> 用 **完整 ref**（`refs/heads/<branch>`）比对而不是 `--short`：存在同名 tag 时 `--short` 会消歧成 `heads/<name>`，纯名字比较会误判并丢掉本来有效的结果。

### 2.2 删除：`remove_worktree_and_branch`（`:641-...`）

这段是安全性的重点，注释极长，核心约束：

1. **路径必须先转绝对路径**再交给 git：
   > Git resolves a `<worktree>` argument by unique path SUFFIX first and only then as a path, so a relative name reaches a registered worktree anywhere on disk and `--force` deletes it.

   实测案例写在测试名里：`a_relative_path_cannot_reach_a_worktree_somewhere_else`。

2. **盘符相对路径直接拒绝**（`is_drive_relative`，`:576`）：
   它的含义依赖每个盘符自己的当前目录，进程和 git 子进程不共享，两边解析结果可能不同——而其中一边是删除。

3. **不是 UTF-8 绝对路径就拒绝**，不用有损转换。

4. **先跑 git，再兜底**：
   ```rust
   git worktree remove --force <abs_path>
   ```
   失败时才走恢复路径（目录已不存在 / 空壳残留 / Windows 下进程占用导致 rmdir 失败）。

5. **`remove_dir` 非递归**：只能删空目录，任何残留文件都会让它失败——这是刻意的「失败关闭」。

6. **删分支前的守卫**：`branch_holds_unlanded_work`（`:526`）——分支 tip 既不是 base 的祖先、也不与 base tree 相同，就说明还有未落地的提交，拒绝 `branch -D`。探针出错一律读作「有工作」，因为删除不可逆。

### 2.3 其他工具

`git_dir` / `rev_parse` / `local_branch_tip` / `has_changes` / `staged_clean` / `is_ancestor` / `merge_base` 等，多为合并与 diff 服务，本次不需要。

---

## 3. 创建流程（`src-tauri/src/work_task/engine.rs:1607-1660`）

```rust
let branch = format!("task/{}", task.id);          // 任务分支名
let dir    = format!("{}-task-{}", basename(&root.path), task.id);
let mut wt_path = worktree_path_in(&root.path, settings.worktree_root.as_deref(), &dir);
let mut branch_used = branch.clone();

if let Err(e) = git_worktree_add(root.path, branch.clone(), wt_path.clone(), Some(start_at)).await {
    // 上一轮残留可能撞名 → 用 run_seq 生成后缀重试一次
    let suffix = format!("r{}b", task.run_seq);
    branch_used = format!("{branch}-{suffix}");
    wt_path = worktree_path_in(&root.path, settings.worktree_root.as_deref(),
                                &format!("{dir}-{suffix}"));
    git_worktree_add(root.path, branch_used.clone(), wt_path.clone(), Some(start_at))
        .await
        .map_err(|_| format!("worktree add failed: {e}"))?;
}

// 在 codeg 的 folder 表里登记这条 worktree，拿到 folder_id
let wt = open_worktree_folder_core(&self.db, wt_path, task.folder_id).await?;

// 把三个事实写回任务行
work_task_service::attach_worktree(
    &self.db.conn, task.id, wt.id, &base_branch, &base_sha, &branch_used,
).await?;
```

**关键时序**：`base_sha` 在**创建 worktree 之前**就已解析并记录（`:1570-1605`），这样创建期间若有人切换了项目分支，diff 基准不会漂移。

**路径命名**（`:7215` `worktree_path_in`）：

```rust
fn worktree_path_in(root_path, worktree_root, name) -> String
// worktree_root 为空 → 与项目同级：<parent>/<repo>-task-<id>
// worktree_root 有值 → <worktree_root>/<repo>-task-<id>
// `~` 展开为 home；相对路径相对 root_path 解析
```

**失败处理**：`worktree add` 两次都失败 → 启动链路标记任务 `failed`，`failure_reason = 'setup_error'`。

**`init_command`**（`:7044` 附近，`SETUP_MARKER = "codeg-task-init-ok"`）：只在新建 worktree 时执行一次，成功写入标记；复用已有 worktree 时跳过。

---

## 4. 清理流程

| 函数 | 位置 | 作用 |
|---|---|---|
| `clear_worktree` | `work_task_service.rs:2558` | 清空任务行上的 worktree 字段 |
| `clear_worktree_by_folder` | `:2600` | 按 folder 反向清理 |
| `tasks_blocking_worktree_removal` | `:2583` | 还有别的任务在用同一个 worktree 时阻止删除 |
| `set_cleanup_state` | `:2526` | 标记 `'failed'`，供 UI 重试 |

**删除时机**：

- 任务**合并落地后**，按 `delete_worktree` 选项决定是否删除
- 崩溃恢复时若 `done` 已写入，按持仓的 `delete_worktree` 补删
- 删除失败 → `cleanup_state = 'failed'`，卡片上出现「清理失败」徽标，可重试
- **绝不因清理失败回滚 `done`**

**删除的分支安全**：删 worktree 后是否连带删 `work_branch`，由 `branch_holds_unlanded_work` 决定（见 §2.2 第 6 点）。

---

## 5. 前端交互

### 5.1 编辑器（`src/components/tasks/task-editor-dialog.tsx`）

```tsx
// 用户请求的分支；"" = 用项目当前分支（历史行为）
const [baseBranch, setBaseBranch] = useState(task?.config?.base_branch ?? "")

// 已经跑过的任务：它的 worktree 与分支是既成事实，不再是设置
const pinned = task != null && task.worktree_folder_id != null

// PR 来源的任务没有分支可选
const choosesBranch = task?.source_kind !== "forge_pr"
```

渲染：

```tsx
{choosesBranch ? (
  <BranchPicker
    folderPath={folderPath}
    value={pinned ? (task?.base_branch ?? baseBranch) : baseBranch}
    onChange={(b) => setBaseBranch(b)}
    defaultFollowsHead      // 空值显示 HEAD，并注明当前分支
    defaultLabel={t("baseBranchDefault")}
    allowRemote={false}     // 合并落回本地 checkout，只能选本地分支
    disabled={folderId == null || pinned}
  />
) : null}
<p className="text-xs text-muted-foreground">{t("baseBranchHint")}</p>
```

**切换文件夹时清空分支**（在 handler 里，不在 effect 里，避免清掉编辑态任务的既有值）：

```tsx
onChange={(id) => { setFolderId(id); setBaseBranch("") }}
```

### 5.2 卡片（`src/components/tasks/task-card.tsx`）

元信息行显示 `文件夹名 / task/1 · 2m`——`work_branch` 用 `font-mono text-[0.625rem]`，分隔符是 `muted-foreground/40`。

### 5.3 分支列表来源

`BranchPicker` 通过 `api` 层调后端列举分支（`allowRemote=false` 时只列本地分支），返回 `{ name, isCurrent }` 之类的结构。

---

## 6. 边界情况（codeg 已处理）

| 情况 | 处理 |
|---|---|
| detached HEAD | 创建后校验 `symbolic-ref`，不是目标分支就回滚 |
| 同名 tag 干扰 | 用完整 ref 比对，不用 `--short` |
| worktree 路径已存在 | 首次失败后用 `run_seq` 后缀重试一次 |
| 相对路径删除 | 先转绝对路径；盘符相对路径直接拒绝 |
| 非 UTF-8 路径 | 拒绝，不做有损转换 |
| Windows 进程占用目录 | 回退到非递归 `remove_dir`，失败则标记可重试 |
| 其他任务共用 worktree | `tasks_blocking_worktree_removal` 阻止删除 |
| 分支有未落地提交 | `branch_holds_unlanded_work` → 保留分支 |
| 删除失败 | `cleanup_state='failed'` + UI 重试入口 |

---

## 7. 最小实现建议（本项目要抄什么、砍什么）

`lightr-vibe` 当前**完全没有 git 能力**（全仓只有一处 `std::process::Command::new("npm")`）。

### 必抄（核心正确性）

| 项 | 理由 |
|---|---|
| `git worktree add <path> <branch>` + `symbolic-ref` 校验 | 防 detached checkout，这是最容易踩且最难排查的坑 |
| 路径转绝对路径再交给 git | 防相对路径误删别处的 worktree |
| `base_sha` 在创建**之前**记录 | 防创建期间分支被切换导致 diff 基准漂移 |
| 删分支前检查未落地提交 | 防误删用户的工作 |
| 创建失败 → 任务 `failed(setup_error)` | 不留半成品状态 |

### 可以砍（本项目的功能面更小）

| codeg 有 | 我们砍掉的理由 |
|---|---|
| `worktree_root` 配置 | 首版固定建在项目旁：`<parent>/<repo>-task-<id>` |
| `init_command` | 按决策采用策略 A：默认不预装、不构建。**保留字段但默认空**，用户可显式填 |
| `run_seq` 后缀重试 | 首版失败即报错，让用户手动处理残留 |
| 远端分支 / PR 基线 | 不涉及 PR 交付 |
| merge / preflight / cleanup 重试状态机 | 没有合并流程，任务结束直接删 |
| `folder` 表登记 worktree | 我们没有 folder 表，直接在任务行存路径字符串 |

### 本项目建议的最小命令集

```rust
// 1. 是否 git 仓库
git -C <root> rev-parse --git-dir

// 2. 当前分支
git -C <root> rev-parse --abbrev-ref HEAD     // 返回 HEAD 视为 detached

// 3. 本地分支列表
git -C <root> for-each-ref --format=%(refname:short) refs/heads

// 4. 基线提交（创建 worktree 之前）
git -C <root> rev-parse <base_branch>

// 5. 创建
git -C <root> worktree prune
git -C <root> worktree add <abs_path> <work_branch>

// 6. 校验挂载
git -C <abs_path> symbolic-ref --quiet HEAD   // == refs/heads/<work_branch>

// 7. 删除
git -C <root> worktree remove --force <abs_path>
git -C <root> worktree prune
git -C <root> branch -D <work_branch>         // 仅在无未落地提交时
```
