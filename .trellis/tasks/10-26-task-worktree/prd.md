# 任务 git worktree 隔离

参考实现：`research/codeg-worktree.md`（codeg 的 worktree 机制，含行号）。

## Goal / 用户价值

当前任务直接在**项目根目录**里跑，带来三个真实问题：

1. **无法并行**：两个任务同时改同一个文件，互相覆盖。
2. **无审核依据**：Agent 改了什么，只能翻会话记录，没有 diff。
3. **无法回滚**：改坏了只能靠 git 手动恢复。

本轮给任务加可选的 git worktree 隔离：勾选后，任务在独立工作树里跑，有自己的分支，
结束即删除，主目录保持干净。

## 范围（用户已确认的四项决策）

- **D1 worktree 开关默认关闭**：新建任务时用户显式勾选才启用；不勾选保持现有行为
  （直接在项目目录跑）。
- **D2 策略 A — 不预装依赖、不自动构建**：worktree 里只有源码。保留可选
  `initCommand` 字段，默认空；用户填了才在新建 worktree 后执行一次。
- **D3 完成后自动删除**：任务走到终态（done / canceled）时删除 worktree 目录。
  删除失败只记录，**不回滚任务状态**，并在详情里给出提示。
- **D4 分支选择**：默认使用项目**当前分支**；用户可从**本地分支**里改选。
  任务自己的分支名固定为 `task/<任务id>`。
- **D5 不限制并发**（用户明确选择）：不做队列、不做并发上限。

## 子任务映射

| # | 子任务 | 目录 | 交付 |
|---|---|---|---|
| 1 | git worktree 后端能力 | `10-26-worktree-backend` | git 命令封装 + 任务表字段 |
| 2 | 编辑器 worktree 与分支选择 | `10-26-worktree-editor` | 开关、分支下拉、校验 |
| 3 | worktree 启动链路与自动清理 | `10-26-worktree-lifecycle` | 创建、以 worktree 为 cwd、删除、回滚 |

顺序：1 → 2 → 3。子任务 2 依赖 1 的分支列表命令；3 依赖 1 的创建/删除命令。

## 跨子任务 Acceptance Criteria

- [ ] **PAC1** 不勾选 worktree 时，行为与改动前**逐字节一致**：仍在项目 rootPath 建会话，无任何 git 调用。
- [ ] **PAC2** 所有 git 调用经 `src-tauri/src/worktree.rs` 单一模块，组件与 store 不直接调 git。
- [ ] **PAC3** 路径安全：交给 git 的 worktree 路径必须是**绝对路径**；盘符相对路径与非 UTF-8 路径拒绝。
- [ ] **PAC4** `base_sha` 在创建 worktree **之前**记录，防止创建期间分支切换导致基准漂移。
- [ ] **PAC5** 创建后用 `symbolic-ref --quiet HEAD` 校验确实挂在 `refs/heads/<work_branch>`，
      否则回滚（删除已创建的 worktree）并报错。
- [ ] **PAC6** 删除时不删有**未落地提交**的分支。
- [ ] **PAC7** 项目目录不是 git 仓库时，worktree 开关禁用并说明原因，不报错。
- [ ] **PAC8** `cargo check` + `cargo test --lib` + `pnpm vue-tsc --noEmit` + `pnpm build` 全绿。

## 磁盘约束（不限制并发的直接后果）

单个 worktree = 一份完整源码副本（`.git` 对象库共享，不重复占空间）。

| 内容 | 占用 |
|---|---|
| 源码 | 几十 MB |
| `node_modules`（若用户手动装） | pnpm 硬链接增量小；npm/yarn 实际占 300~500 MB |
| `target/`（若用户手动构建） | 每个 2~5 GB |

因为不预装不构建，默认峰值可控。但**用户若在多个 worktree 里手动执行 `pnpm install` /
`cargo build`，磁盘会快速上涨**。这一点必须在编辑器说明文案里写清楚。

## Out of Scope

- 合并回主分支、diff 审核视图、PR 交付
- preflight 验收命令
- `worktree_root` 自定义目录（固定建在项目旁）
- 远端分支
- 并发队列与磁盘配额

## 验收方式

人工在 `pnpm dev:tauri` 下：
1. 不勾选 worktree 建任务 → 确认行为与之前一致
2. 勾选 worktree 建任务 → `git worktree list` 能看到新条目，会话 cwd 在 worktree 内
3. 任务完成 → worktree 目录消失，`git worktree list` 恢复原状
4. 项目根目录 `git status` 全程干净
