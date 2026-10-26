# Design: 任务 git worktree 隔离

需求 `prd.md`，codeg 参考实现 `research/codeg-worktree.md`。
桥接规范：`.trellis/spec/frontend/tauri-bridge.md`。

## 1. 与 codeg 的差异（先看这个）

| 点 | codeg | 本项目 | 理由 |
|---|---|---|---|
| worktree 登记 | `folder` 表里建一条记录，任务存 `worktree_folder_id` | 任务行直接存 `worktreePath: string` | 我们没有 folder 表，为一条路径建表不划算 |
| 路径 | `worktree_root` 可配置 | 固定 `<项目父目录>/<repo>-task-<id>` | 首版不引入文件夹设置 |
| 分支 | `task/<id>` + `run_seq` 后缀重试 | `task/<id>`，失败即报错 | 残留由用户手动 `git worktree prune` |
| init 命令 | 文件夹级 `init_command` | 任务级 `initCommand`（可选） | 我们没有文件夹设置表 |
| 合并 | merge pump、`merge_state` | 无 | Out of Scope |
| 删除时机 | 合并后按选项 | 进终态即删 | 无合并流程 |

**必须照抄的三条安全约束**（见 §4）：绝对路径、创建前记 `base_sha`、`symbolic-ref` 校验。

## 2. 后端：`src-tauri/src/worktree.rs`（新增）

### 2.1 命令集

```rust
/// 仓库信息：是否 git 仓库 + 当前分支 + 本地分支列表
#[tauri::command]
pub fn worktree_repo_info(root_path: String) -> Result<RepoInfo, String>

#[tauri::command]
pub fn worktree_create(req: CreateWorktreeRequest) -> Result<CreatedWorktree, String>

#[tauri::command]
pub fn worktree_remove(root_path: String, worktree_path: String,
                       work_branch: String, base_branch: Option<String>,
                       base_sha: Option<String>) -> Result<(), String>
```

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoInfo {
    /// false → 前端禁用 worktree 开关
    pub is_repo: bool,
    /// 当前分支名；detached HEAD 时为 None
    pub current_branch: Option<String>,
    /// 本地分支（不含远端）
    pub branches: Vec<String>,
    /// 非仓库 / 无提交时的可读原因
    pub detail: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorktreeRequest {
    pub root_path: String,
    /// 空 = 用当前分支
    #[serde(default)]
    pub base_branch: Option<String>,
    pub task_id: String,
    /// 可选初始化命令；空则不执行（策略 A）
    #[serde(default)]
    pub init_command: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedWorktree {
    pub worktree_path: String,
    pub work_branch: String,
    /// 创建前解析的基线提交
    pub base_sha: String,
    /// 实际使用的基线分支（空请求解析为当前分支后的结果）
    pub base_branch: String,
    /// init 命令失败时的输出尾部；成功为 None
    #[serde(skip_serializing_if = "Option::is_none")]
    pub init_error: Option<String>,
}
```

### 2.2 内部实现要点

**git 调用统一走一个 helper**：

```rust
fn run_git(cwd: &str, args: &[&str]) -> Result<Output, String>
```
- 用 `std::process::Command::new("git")`
- Windows 下加 `CREATE_NO_WINDOW`（与 `agent.rs` 的既有做法一致）
- 失败时把 stderr 尾部拼进错误串：`git <子命令>: <stderr 尾部>`

**`worktree_repo_info`**：

```
git -C <root> rev-parse --git-dir        // 失败 → is_repo=false + detail
git -C <root> rev-parse --abbrev-ref HEAD // "HEAD" → detached，current_branch=None
git -C <root> for-each-ref --format=%(refname:short) refs/heads
```

**`worktree_create`（严格按此顺序）**：

```
1. base_branch = req.base_branch 或 (repo_info.current_branch)
2. base_sha = git rev-parse <base_branch>          ← 必须在创建之前
3. work_branch = "task/<task_id>"
4. worktree_path = <root 的父目录>/<root 目录名>-task-<task_id>   （转绝对路径）
5. 若 worktree_path 已存在 → Err("worktree-path-exists")
6. git worktree prune
7. git worktree add -b <work_branch> <abs_path> <base_branch>
     失败 → 回滚本次新建的分支 → Err("worktree-add: <stderr>")
8. git -C <abs_path> symbolic-ref --quiet HEAD
     期望 == "refs/heads/<work_branch>"
     不等 → git worktree remove --force; prune; 无未落地提交则删分支;
            Err("worktree-detached")
9. init_command 非空 → 在该目录跑一次；失败只记 init_error，不回滚
10. 返回 CreatedWorktree
```

> 第 7 步为什么必须带 `-b`（design 初稿写错，实现时经实测修正）：
> `task/<id>` 是新分支，`worktree add <path> <name>` 要求 `<name>` 已存在，否则
> `fatal: invalid reference`；若恰有同名 tag 则静默 detached。`-b` 显式指定起点为
> `base_branch`，同时消除了「创建期间分支漂移」的窗口。
>
> `-b` 的**副作用**：git 先建分支、后建目录。目录阶段失败会残留分支，重试必然撞
> `a branch named '<x>' already exists`，任务从此无法启动。因此失败时必须回滚该分支
> （仅在「调用前分支不存在」时才删，避免误删用户分支）。
>
> 第 8 步是最后一道兜底：即便 `-b` 正常，"HEAD 是否真挂在目标分支" 仍需校验——
> detached checkout 上的提交会挂到没人指向的地方。

**`worktree_remove`**：

```
1. 拒绝非绝对路径 / 盘符相对路径 → Err("path-not-absolute")
2. git -C <root> worktree remove --force <abs_path>
     失败 → 若目录已不存在，视为成功
3. git -C <root> worktree prune
4. 删分支前检查：work_branch 的 tip 是否为 base 的祖先或 tree 相同
   否 → 保留分支（不删），返回成功
   是 → git -C <root> branch -D <work_branch>
```

### 2.3 任务字段（`src-tauri/src/task.rs`）

`WorkTask` 新增（全部 `#[serde(default)]`，旧数据可读）：

```rust
pub use_worktree: bool,              // 是否启用（默认 false）
pub worktree_path: Option<String>,   // 创建后的绝对路径
pub work_branch: Option<String>,     // task/<id>
pub base_branch: Option<String>,
pub base_sha: Option<String>,
pub init_command: Option<String>,    // 可选，默认 None
```

`TaskDraft` / `TaskPatch` 同步加 `use_worktree` / `base_branch` / `init_command`
（**不**含 `worktree_path` / `work_branch` / `base_sha`——那三个由创建流程写入，不接受客户端伪造）。

新增命令：

```rust
task_attach_worktree { id, worktreePath, workBranch, baseBranch, baseSha }
task_clear_worktree  { id }
```

## 3. 前端

### 3.1 类型（`src/types/task.ts`）

```ts
export interface WorkTask {
  // ...existing
  useWorktree: boolean
  worktreePath: string | null
  workBranch: string | null
  baseBranch: string | null
  baseSha: string | null
  initCommand: string | null
}

export interface TaskDraft {
  // ...existing
  useWorktree?: boolean
  baseBranch?: string | null
  initCommand?: string | null
}
```

### 3.2 service（`src/services/worktreeService.ts` 新增）

按 `isTauri()` 双通道；浏览器 dev 的 mock 返回
`{ isRepo: true, currentBranch: 'main', branches: ['main'] }`，创建返回假路径，
便于 UI 调试。

### 3.3 编辑器（`src/views/tasks/components/TaskEditorDialog.vue`）

在「目标」区下方新增：

```
☐ 使用 git worktree 隔离
   在独立工作树里执行，不影响当前目录；任务结束时自动删除。
   分支： [ main ▾ ]
   （可选）初始化命令： [                    ]  例：pnpm install
```

规则：

- `repoInfo.isRepo === false` → 开关禁用，说明「当前项目不是 git 仓库」
- 开关**默认关闭**（`useWorktree: false`），与既有流程完全一致
- 只有开关打开时才显示分支下拉与 init 命令输入
- 分支下拉默认显示当前分支；`currentBranch` 为 null（detached）时提示「当前处于游离 HEAD，请选择分支」
- 切换项目时重置 `baseBranch` 与 `useWorktree`

### 3.4 启动链路（`src/views/tasks/taskSessionBridge.ts`）

```
1. transition(id, 'queued', 'todo')
2. 若 task.useWorktree：
     info = await worktreeService.create({ rootPath: project.rootPath, baseBranch, taskId, initCommand })
     await taskStore.attachWorktree(id, info)      // 写回路径/分支/sha
     cwd = info.worktreePath
   否则 cwd = project.rootPath
3. createDraft({ kind:'folder', projectId, cwd, engineId })
4. attachSession → preparing → running → openSession → setModel/setConfigOption → send
5. 失败回滚：若已创建 worktree，先 worktreeService.remove 再回退任务状态
```

### 3.5 清理（终态触发）

在 `taskStore` 的状态转移成功后判断：`status ∈ {done, canceled}` 且 `worktreePath != null`
→ 调 `worktreeService.remove(...)`，成功后 `task_clear_worktree`。

失败处理：记 `lastError`，**不回滚状态**；详情里显示「工作树清理失败」与路径，便于用户手动处理。

## 4. 安全约束（照抄 codeg 的三条）

1. **绝对路径**：git 的 `<worktree>` 参数先按路径后缀匹配，相对路径可能命中磁盘上别处的
   同名 worktree 并被 `--force` 删除。必须在 Rust 侧转绝对路径再传。
2. **创建前记 `base_sha`**：创建期间若有人在项目目录切分支，基准会漂移。
3. **`symbolic-ref` 校验**：见 §2.2 第 8 步。

额外：盘符相对路径（如 `C:foo`）与非 UTF-8 路径**直接拒绝**。

## 5. 兼容 / 回滚

- 新增文件：`src-tauri/src/worktree.rs`、`src/services/worktreeService.ts`
- 数据层：`task.rs` 加 6 个字段（`#[serde(default)]`，旧 `tasks.json` 读入为默认值）
- 前端：`types/task.ts`、`services/taskService.ts`、`taskMock.ts`、`stores/task.ts`、
  `TaskEditorDialog.vue`、`taskSessionBridge.ts`
- 回滚 = `git checkout` 上述文件 + 删除两个新文件；`tasks.json` 里的新字段对旧代码无害

## 6. 风险

| 风险 | 缓解 |
|---|---|
| 不限制并发 → 多 worktree 堆积占盘 | 任务终态即删；编辑器文案说明「worktree 内未预装依赖，装/构建会占额外空间」 |
| Windows 下目录被进程占用导致删除失败 | 失败不回滚状态，详情提示路径 + 手动处理入口 |
| 用户在 worktree 里手动构建导致 GB 级占用 | 同上，文案提示；后续可加磁盘占用显示 |
| `git` 不在 PATH | `worktree_repo_info` 返回 `is_repo:false` + detail，开关禁用 |
| 项目有未提交改动 | `worktree add` 不受影响（它从提交拉出，不是工作区） |
| 同名分支已存在 | `worktree add` 失败，报错并提示用户先处理 |
