# PRD: 编辑器 worktree 与分支选择

父任务：`10-26-task-worktree`。前置：`10-26-worktree-backend`。
设计：`../10-26-task-worktree/design.md` §3.3。参考：`research/codeg-worktree.md` §5。

## Goal

用户新建任务时能一眼决定「要不要隔离」以及「从哪个分支开始」。默认不开，不打扰既有流程。

## Requirements

### R1 开关（默认关闭）

- 「目标」区下方新增一块 worktree 设置。
- 开关文案：「使用 git worktree 隔离」，默认**不勾选**。
- 说明文字：「在独立工作树里执行，不影响当前目录；任务结束时自动删除。」
- 磁盘提示：「worktree 内不预装依赖，安装或构建会占用额外磁盘空间。」

### R2 分支选择

- 仅在开关打开时显示。
- 下拉默认显示**当前分支**。
- 列出**本地分支**（来自 `worktree_repo_info.branches`）。
- `currentBranch` 为 `null`（detached HEAD）时提示「当前处于游离 HEAD，请选择一个分支」，且不允许留空。

### R3 可选初始化命令

- 仅在开关打开时显示。
- 输入框占位：`例：pnpm install`。
- 空 = 不执行（策略 A 的默认）。

### R4 非 git 项目

- `repoInfo.isRepo === false` → 整块禁用，显示原因（来自 `detail`）。
- 不抛错、不阻塞任务创建。

### R5 提交

- `submit` payload 增加 `useWorktree` / `baseBranch` / `initCommand`。
- 两个调用方（`TasksView` / `ProjectPickerView`）透传到 `taskStore.create` / `update`。

### R6 编辑模式

- 已创建 worktree 的任务：项目、分支、开关全部**只读**（既成事实）。
- 详情抽屉显示 worktree 路径与分支（只读文本 + 可复制）。

### R7 切换项目

- 切换项目时重置 `useWorktree=false`、`baseBranch=null`（分支属于具体仓库）。

## Acceptance Criteria

- [ ] **AC1** 新开编辑器，worktree 开关默认不勾选，分支区不显示。
- [ ] **AC2** 勾选开关后出现分支下拉与 init 命令输入。
- [ ] **AC3** 分支下拉默认选中的是项目当前分支。
- [ ] **AC4** 下拉列出所有本地分支（与 `git branch` 一致，不含远端）。
- [ ] **AC5** detached HEAD 时给出提示且不能提交空分支。
- [ ] **AC6** 非 git 项目：开关禁用，显示原因。
- [ ] **AC7** 切换项目后，之前选的分支被重置。
- [ ] **AC8** 提交后任务行带上了 `useWorktree` / `baseBranch` / `initCommand`。
- [ ] **AC9** 已创建 worktree 的任务，编辑器里项目/分支/开关只读。
- [ ] **AC10** 详情抽屉显示 worktree 路径与分支。
- [ ] **AC11** 不勾选 worktree 时提交的 payload 与改动前等价（三个新字段为默认值）。
- [ ] **AC12** `vue-tsc --noEmit` + `pnpm build` 绿。

## Out of Scope

- worktree 的实际创建与删除（子任务 3）
- 远端分支选择
- 分支新建（只能选已有分支）
