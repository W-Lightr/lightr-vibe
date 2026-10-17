# PRD: 任务数据层与 Rust 命令

父任务：`10-17-task-board`。设计：`../10-17-task-board/design.md` §2/§3。

## Goal

让「任务」成为可持久化、可原子变更的一等数据：类型契约、JSON 落盘、状态机校验、
前端双通道 service 与 Pinia store。本子任务**不做任何 UI**。

## Requirements

### R1 类型契约
- `src/types/task.ts` 导出 `TaskStatus` / `WorkTask` / `WorkTaskEvent` / `TaskColumnId`
  与错误串常量，字段与 `design.md` §2.1 逐字一致。

### R2 持久化
- 文件 `~/.lightr-vibe/lightr-vibe/tasks.json`，格式 `design.md` §2.2。
- 读：文件不存在 → 空数据，非错误；JSON 损坏 → 返回空数据 + `warn` 日志，**不覆盖原文件**
  （下次写入前把损坏文件备份为 `tasks.json.bak`）。
- 写：整体原子写（复用 `fs_write_text` 语义）。
- 事件每任务截断到 200 条。

### R3 命令集（`src-tauri/src/task.rs`）

| 命令 | 入参 | 行为 |
|---|---|---|
| `task_list` | `projectId?: string` | 返回全部任务（可选按项目过滤） |
| `task_create` | `draft: TaskDraft` | 追加到该 `projectId` 列尾，`status='todo'` |
| `task_update` | `id, patch` | 仅 `todo`/`failed` 允许改 `title`/`prompt`/`engineId` |
| `task_delete` | `id` | 硬删任务 + 其事件 |
| `task_transition` | `id, to, expectedFrom` | 见 `design.md` §3.2 |
| `task_reorder` | `projectId, orderedIds` | 按 index 写 `sortOrder`，作用域校验 |
| `task_attach_session` | `id, sessionId, engineId` | 绑定会话（启动链路用） |
| `task_archive` | `id, archived` | 仅终态（done/canceled/failed）可归档 |
| `task_events` | `id` | 时间线升序 |

### R4 状态机
- 转换表见 `design.md` §3.1，作为 Rust 常量表 + `can_transition` 纯函数。
- 非法转换返回 `illegal-transition: <from>-><to>`；`expectedFrom` 不匹配返回 `stale`。
- 进入 `queued` 时 `runSeq += 1`；进入 `review`/`failed` 写 `settledAt`；进入
  `done`/`canceled` 写 `finishedAt`；进入 `queued` 时若 `startedAt` 为空则写 `startedAt`。
- 每次转换写一条 `status_changed` 事件，`payload = { from, to }`。

### R5 双通道 service
- `src/services/taskService.ts` 全部方法：`isTauri()` 分支 → `invoke('task_*', {...})`
  / `taskMock.*`。参数用 camelCase，与 Rust `rename_all = "camelCase"` 对齐。
- `taskMock.ts` 内存实现同一状态机（防止浏览器 dev 出现非法状态），并带 3 条演示任务。

### R6 store
- `src/stores/task.ts`（Pinia setup store）：`tasks`、`loaded`、`load()`、`create()`、
  `update()`、`remove()`、`transition()`、`reorder()`、`archive()`、`attachSession()`、
  `byProject(projectId)`、`eventsOf(id)`。
- 所有变更方法 await service 后用返回值更新本地数组（不做乐观更新）。

## Acceptance Criteria

- [ ] **AC1** `cargo check` + `cargo test --lib` 绿；`can_transition` 有穷举单测。
- [ ] **AC2** `task_transition` 对 `done → todo`、`todo → review` 返回 `illegal-transition`。
- [ ] **AC3** `expectedFrom` 不匹配返回 `stale`，且不修改行。
- [ ] **AC4** `task_reorder` 传别的项目的 `id` 不报错也不改动（作用域过滤）。
- [ ] **AC5** 事件截断：单任务写 250 次转换后 `eventsOf` 只剩 200 条。
- [ ] **AC6** `vue-tsc --noEmit` + `pnpm build` 绿。
- [ ] **AC7** 浏览器 dev（`pnpm dev`）下列表能读到 mock 演示任务。
- [ ] **AC8** 组件层零 `invoke`（`rg "invoke\(" src/views` 无命中）。

## Out of Scope

- 任何 UI / 路由 / 拖拽
- 会话创建与 prompt 发送（子任务 3）
- 多窗口并发写（单 `Mutex` 已覆盖单进程）
