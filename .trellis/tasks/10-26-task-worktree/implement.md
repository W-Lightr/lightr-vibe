# Implement: 任务 git worktree 隔离

需求 `prd.md`，设计 `design.md`，codeg 参考 `research/codeg-worktree.md`。

顺序：子任务 1（后端）→ 2（编辑器）→ 3（启动与清理）。

## 验证命令

```powershell
cargo check                       # workdir: src-tauri
cargo test --lib                  # workdir: src-tauri
pnpm vue-tsc --noEmit
pnpm build
```

## 1 · 子任务 `10-26-worktree-backend`

- [x] 新建 `src-tauri/src/worktree.rs`
  - [x] `run_git(cwd, args)` helper：`CREATE_NO_WINDOW`，错误带 stderr 尾部
  - [x] `worktree_repo_info`：`rev-parse --git-dir` / `--abbrev-ref HEAD` /
        `for-each-ref refs/heads`
  - [x] `worktree_create`：严格按 design §2.2 的 10 步
    - [x] `base_sha` 在 `worktree add` **之前**解析
    - [x] 路径转绝对路径；盘符相对路径 / 非 UTF-8 拒绝
    - [x] `symbolic-ref --quiet HEAD` 校验，失败回滚
    - [x] `init_command` 可选，失败只记 `init_error`
  - [x] `worktree_remove`：绝对路径校验 → `worktree remove --force` →
        `prune` → 未落地提交检查 → `branch -D`
  - [x] Rust 单测：绝对路径判定、`is_drive_relative`、分支未落地检查
- [x] `src-tauri/src/task.rs`
  - [x] `WorkTask` 加 `use_worktree` / `worktree_path` / `work_branch` /
        `base_branch` / `base_sha` / `init_command`（全部 `#[serde(default)]`）
  - [x] `TaskDraft` / `TaskPatch` 加 `use_worktree` / `base_branch` / `init_command`
  - [x] 新命令 `task_attach_worktree` / `task_clear_worktree`
  - [x] 补 camelCase 序列化断言
- [x] `lib.rs` 注册 5 个命令
- [x] 前端契约层：`types/task.ts`、`taskService.ts`、`taskMock.ts`、`stores/task.ts`
- [x] 新建 `src/services/worktreeService.ts`（双通道 + mock）

**回滚点**：删除 `worktree.rs` 与 `worktreeService.ts`；`git checkout src-tauri/src/task.rs src-tauri/src/lib.rs src/types/task.ts src/services/taskService.ts src/services/taskMock.ts src/stores/task.ts`

## 2 · 子任务 `10-26-worktree-editor`

- [ ] `TaskEditorDialog.vue` 「目标」区下方新增 worktree 区
  - [ ] 开关（默认关）+ 说明文案（含磁盘提示）
  - [ ] 分支下拉：默认当前分支，列出本地分支，detached 时提示
  - [ ] 可选 initCommand 输入（占位 `例：pnpm install`）
  - [ ] `repoInfo.isRepo === false` → 整块禁用 + 原因
  - [ ] 仅开关打开时显示分支与 init 输入
  - [ ] 切换项目重置 `useWorktree` / `baseBranch`
- [ ] submit payload 带上 `useWorktree` / `baseBranch` / `initCommand`
- [ ] `ProjectPickerView.vue` / `TasksView.vue` 的 `onEditorSubmit` 透传三个新字段
- [ ] 详情抽屉显示 worktree 路径与分支（只读）

**回滚点**：`git checkout src/views/tasks/components/TaskEditorDialog.vue src/views/tasks/components/TaskDetailDialog.vue src/views/tasks/TasksView.vue src/views/tasks/ProjectPickerView.vue`

## 3 · 子任务 `10-26-worktree-lifecycle`

- [ ] `taskSessionBridge.ts` 启动链路
  - [ ] `useWorktree` 为真 → `worktreeService.create` → `attachWorktree` → `cwd = worktreePath`
  - [ ] 为假 → `cwd = project.rootPath`（**保持原路径逐字节不变**）
  - [ ] 创建失败 → 任务 `failed(setup_error)`，不建会话
  - [ ] 后续步骤失败且已建 worktree → 先删再回退
- [ ] 终态清理
  - [ ] `done` / `canceled` 且 `worktreePath != null` → `worktreeService.remove` → `clearWorktree`
  - [ ] 删除失败：写 `lastError`，**不回滚状态**
- [ ] 详情抽屉显示「清理失败」提示与路径
- [ ] 确认不勾选 worktree 的路径零回归

**回滚点**：`git checkout src/views/tasks/taskSessionBridge.ts src/views/tasks/components/TaskDetailDialog.vue`

---

## 关键约束

- **git 只在 `worktree.rs` 里调用**（PAC2）。组件、store、service 都不直接 spawn git。
- **绝对路径**再交给 git（PAC3）；`base_sha` 在创建前记录（PAC4）；
  `symbolic-ref` 校验（PAC5）；不删有未落地提交的分支（PAC6）。
- **不勾选 worktree = 零变化**（PAC1），这是最容易回归的地方，改完必须逐行对比启动链路。
- 默认不预装依赖、不构建（策略 A）；`initCommand` 默认空。
- PowerShell `Get-Content`/`Set-Content` 会破坏本仓库 UTF-8 中文，用 edit/write 工具或 Node/Python。

## 人工验收

- [ ] 不勾选 worktree：建任务 → 行为与之前完全一致，`git worktree list` 无新条目
- [ ] 勾选 worktree + 默认分支：建任务 → `git worktree list` 出现 `<repo>-task-<id>`
- [ ] 会话目录正确：让 Agent 输出 `pwd`，落在 worktree 内
- [ ] 项目根目录 `git status` 全程干净
- [ ] 任务完成后：worktree 目录消失，分支 `task/<id>` 已删
- [ ] 选其他本地分支：worktree 基于该分支创建
- [ ] 非 git 项目：开关禁用且有说明
- [ ] 手工制造删除失败（如占用目录）：状态不回滚，详情有提示
