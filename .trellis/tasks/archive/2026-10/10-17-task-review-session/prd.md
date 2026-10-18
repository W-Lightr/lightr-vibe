# PRD: 任务审核、详情与会话联动

父任务：`10-17-task-board`。前置：`10-17-task-data-layer`、`10-17-task-board-ui`。
设计：`../10-17-task-board/design.md` §7。

## Goal

让任务真正「跑起来」：从看板启动一个任务会创建一个绑定该项目的 ACP 会话并发送提示词，
Agent 干完后任务自动进入「审核」，用户看结果后通过或打回。

## Requirements

### R1 编辑器 `TaskEditorDialog.vue`
- 新建：项目选择（必填）、标题、提示词、引擎选择（默认取 `workspace.selectedModel` 对应引擎）。
- 编辑：仅 `todo` / `failed` 可改标题、提示词、引擎。
- 校验：标题非空；未选项目禁用提交。

### R2 详情 `TaskDetailDialog.vue`
- 头部：标题、状态 chip、项目名、创建/开始/结算时间。
- 正文：提示词全文、`lastError`（若有）、`resultSummary`（若有）。
- 时间线：`task_events` 升序，`status_changed` 渲染为「待办 → 进行中」。
- 动作区：按状态给全量动作（启动 / 停止 / 通过 / 打回 / 重试 / 归档 / 删除 / 断开会话）。
- 「查看会话」：`workspace.openSession(sessionId)` + `router.push('/coding')`。

### R3 启动链路
顺序（任一步失败则回退并提示）：
1. `transition(id, 'queued', 'todo')`
2. `sessionStore.createDraft(model, { kind:'folder', projectId, cwd, engineId })`
3. `taskService.attachSession(id, sessionId, engineId)`
4. `transition(id, 'preparing', 'queued')` → `transition(id, 'running', 'preparing')`
5. `workspace.openSession(sessionId)`
6. 发送 `task.prompt`

### R4 停止
- `useConversation.stop(sessionId)` 后任务保持 `running`（不判失败），提示「已停止，可打回或通过」。

### R5 turn 回流
- `useConversation` 新增可选回调 `onTurnSettled(payload)`：
  `{ sessionId, ok, errorKind?, startedAt }`，**默认不传则行为完全不变**。
- task store 订阅：
  - `ok` → 该 `sessionId` 对应且状态为 `running` 的任务 → `review`
  - `errorKind === 'empty'` → `failed` + `failureReason='empty_turn'`
  - 其他错误 → `failed` + `failureReason='agent_error'`
  - 找不到对应任务或状态不是 `running` → 静默忽略
- 用 `settledAt` / `startedAt` 与任务 `runSeq` 双重防串（迟到回调不改新状态）。

### R6 审核动作
- 通过：`review → done`，写 `finishedAt`。
- 打回：`review → todo`，保留 `sessionId`（下次启动续用原会话），清 `settledAt`/`resultSummary`。
- 失败重试：`failed → queued` 后重新走 R3 的 3–6 步（复用 `sessionId` 走 load）。

### R7 mock
- `taskMock` 的 turn 回流在浏览器 dev 用手动按钮模拟（详情里的「模拟完成 / 模拟失败」，
  仅 `!isTauri()` 时显示）。

## Acceptance Criteria

- [x] **AC1** `cargo check` + `cargo test --lib` + `vue-tsc --noEmit` + `pnpm build` 全绿。
- [x] **AC2** 新建任务 → 出现在「待办」列；点开始 → 卡片进入「进行中」且右侧出现会话标签。
- [x] **AC3** Agent 正常结束 → 卡片自动进入「审核」列。
- [x] **AC4** Agent 报空回复（`errorKind:'empty'`）→ 卡片进入「审核」列并以失败样式显示
      `failureReason='empty_turn'`；其他错误同理。
- [x] **AC5** 「通过」→ 已完成列；「打回」→ 待办列且详情里保留了会话入口。
- [x] **AC6** 详情时间线按时间升序显示全部状态变更。
- [x] **AC7** 不传 `onTurnSettled` 时 `useConversation` 行为与改动前逐字节一致（opencode 零回归）。
- [x] **AC8** 项目被移除后，其任务在「全部项目」视图可见、可按「项目已移除」标记，且可删除/归档。

## Out of Scope

- 自动审核 / Agent 自报 verdict
- 合并、worktree、PR（见父任务 `research/codeg-gap.md`）
- 通知推送
