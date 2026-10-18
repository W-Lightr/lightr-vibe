# 任务看板（待办 / 进行中 / 审核 / 已完成）

顶层任务。参考实现：`H:\AI\Light-vibe\codeg`（Rust + Next.js，路径与行号见 `research/codeg-task-board.md`）。

## Goal / 用户价值

用户已在侧栏加入若干项目文件夹（`Project`，`src/types/project.ts:6`），但当前项目没有任何
「任务」概念——无法把一个待办事项挂到某个项目上、跟踪它从待办到完成、也没法审核 Agent 干完
的活。

本任务补齐这条链路：**按已添加的项目文件夹创建任务 → 四列看板（待办 / 进行中 / 审核 /
已完成）→ 拖拽推进状态 → 任务与 ACP 会话联动 → 审核通过或打回。**

## 范围（用户已确认）

四项决策（来自需求澄清）：

- **D1 持久化用 JSON 文件**，不引入 SQLite / SeaORM。任务索引落在
  `~/.lightr-vibe/lightr-vibe/tasks.json`（遵循 `utils/appData.ts` 命名空间硬规则）。
- **D2 列数固定 4 列**（照 codeg `board-columns.ts:22`）：`todo` / `inProgress` /
  `attention` / `done`。失败与等待输入归入「审核」列，用状态 chip 区分，不额外开列。
- **D3 跨列拖拽只允许三条**（其余由服务端拒绝并给出原因）：`todo → queued`（开始）、
  `review → done`（通过）、`review → todo`（打回）。同列内自由排序。
- **D4 一个任务 = 一个独立 ACP 会话**（`sessionId` 一对一），不允许多任务共享会话。

## 子任务映射（非依赖系统；排序见各子任务 implement.md）

| # | 子任务 | 目录 | 交付 |
|---|---|---|---|
| 1 | 任务数据层与 Rust 命令 | `10-17-task-data-layer` | 类型契约、JSON 持久化、`task_*` 命令、状态转移 CAS、双通道 service |
| 2 | 看板 UI 与拖拽 | `10-17-task-board-ui` | 四列页面、卡片、指针拖拽、项目筛选 |
| 3 | 任务审核、详情与会话联动 | `10-17-task-review-session` | 详情抽屉、通过/打回、启动并驱动会话、turn 回流 |

实施顺序 1 → 2 → 3（2 依赖 1 的类型与 service，3 依赖 2 的卡片入口）。

## 跨子任务 Acceptance Criteria

- [ ] **PAC1** 组件与 store 不直接 `invoke`；所有命令经 `src/services/taskService.ts` 单一入口，
      且带 `isTauri()` 双通道（`.trellis/spec/frontend/tauri-bridge.md`）。
- [ ] **PAC2** 类型契约单一来源：`src/types/task.ts`，Rust 侧 `#[serde(rename_all = "camelCase")]` 字段逐字对齐。
- [ ] **PAC3** 状态转移的服务端校验是唯一权威：前端只做交互提示，非法转移由 Rust 拒绝并返回可读原因。
- [ ] **PAC4** 项目删除 / 项目在筛选器里消失时，看板不崩：任务按 `projectId` 归属，失去项目的任务
      在「全部项目」视图下仍可见并标记「项目已移除」。
- [ ] **PAC5** 浏览器 dev（非 Tauri）不崩：`taskMock` 内存实现与真实通道同形。
- [ ] **PAC6** `cargo check` + `cargo test --lib`（`src-tauri/`）、`vue-tsc --noEmit`、`pnpm build` 全绿。
- [ ] **PAC7** 不改动既有 ACP 会话路径的行为（opencode / claude / codex 零回归）。

## 与 codeg 的差异（本次明确不实现）

对齐 codeg 时被刻意砍掉的能力，写在 `research/codeg-gap.md`，子任务落地后统一复核一次：

- git worktree 隔离（`work_branch` / `base_sha` / `cleanup_state`）
- 合并落地与 merge pump 串行队列（`merge_state` / `pending_merge` / `merging` 列）
- PR 交付（`deliver_pr` / `source_kind = forge_pr`）
- preflight 验收命令（红绿灯）
- 定时启动（`scheduled_at`）、自动认领（`auto_process` / `max_concurrent`）、自动合并（`auto_merge`）
- 任务模板、forge issue/PR 导入、文件夹级 settings 表
- 失败后 `requeue` 的 resurrection 去重守卫
- 崩溃恢复（`boot_reconcile_interrupted`）

## Out of Scope

- 多用户 / 远端同步 / 服务器模式
- 任务依赖关系图、里程碑、标签体系
- 通知（系统通知、状态栏角标）
