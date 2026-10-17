# Implement: 任务看板（父任务）

顺序：子任务 1 → 2 → 3。每个子任务结束时四门禁全绿，并在本文件勾选对应行。

## 验证命令

```powershell
cargo check                 # workdir: src-tauri
cargo test --lib            # workdir: src-tauri
pnpm vue-tsc --noEmit
pnpm build
```

## 0 · 准备（一次性）

- [ ] 建目录 `src/views/tasks/`、`src/views/tasks/components/`
- [x] `src/utils/appData.ts` 加 `tasksIndexFileSync()`
- [ ] `.trellis/spec/frontend/directory-structure.md` 补 `views/tasks/` 一节（子任务 3 末尾统一补）

## 1 · 子任务 `10-17-task-data-layer`

- [x] `src/types/task.ts`：`TaskStatus`、`WorkTask`、`WorkTaskEvent`、`TaskNode`、错误串常量
- [x] `src-tauri/src/task.rs`：
  - [x] `TaskStore { path, data: Mutex<TaskFile> }` + `load/save`
  - [x] `TRANSITIONS` 常量表 + `can_transition(from, to)` 纯函数（带 `#[test]` 穷举）
  - [x] 命令：`task_list` / `task_create` / `task_update` / `task_delete` /
        `task_transition` / `task_reorder` / `task_events` / `task_archive` /
        `task_attach_session`
  - [x] `lib.rs` 注册全部命令
- [x] `src/services/taskService.ts` + `src/services/taskMock.ts`
- [x] `src/stores/task.ts`：`load` / `create` / `update` / `remove` / `transition` /
      `reorder` / `archive` / `byProject` / `eventsOf`
- [x] Rust 单测：合法/非法转移、stale 检测、reorder 作用域、事件截断
- [x] mock 演示数据（浏览器 dev 有内容可看）

**回滚点**：删除上述新文件 + `lib.rs` 注册行 + `appData.ts` 一个函数。

## 2 · 子任务 `10-17-task-board-ui`

- [ ] `src/views/tasks/boardColumns.ts`（`STATUSES_BY_COLUMN` / `columnForStatus` /
      `groupTasksByColumn` / `allowedDrop`）
- [ ] `router/index.ts` 加 `/tasks`
- [ ] `TopBar.vue` 加「任务」入口
- [ ] `TasksView.vue`：工具栏 + 四列容器
- [ ] `TaskColumn.vue` / `TaskCard.vue`
- [ ] 拖拽：`useTaskDrag.ts` composable（pointerdown/move/up + Teleport 预览 + 落列判定）
- [ ] 空态、加载态、项目筛选、隐藏已取消/已归档

**回滚点**：删除 `src/views/tasks/` + 路由一行 + TopBar 按钮块。

## 3 · 子任务 `10-17-task-review-session`

- [ ] `TaskDetailDialog.vue`：标题/提示词/时间线/会话入口/动作区
- [ ] `TaskEditorDialog.vue`：项目选择 + 标题 + 提示词 + 引擎选择
- [ ] 启动链路：创建工作区会话 → attach → 发送 prompt → running
- [ ] `useConversation` 加 `onTurnSettled` 可选回调（不改变现有行为）
- [ ] task store 订阅回调 → `running → review|failed`
- [ ] 审核：通过（`review → done`）、打回（`review → todo`）
- [ ] 归档、删除、失败重试
- [ ] spec 更新：`tauri-bridge.md`（新增 task 命令契约）、`directory-structure.md`、`state-management.md`

**回滚点**：子任务 3 的改动集中在新增组件 + `useConversation` 一个可选钩子。

---

## 关键契约（所有子任务共用）

- **状态转移只在 Rust 侧校验**；前端 `allowedDrop` 只是交互提示。
- **组件不 invoke**；一切经 `taskService`。
- **`projectId` 是字符串**（`proj-xxxx`），不要照抄 codeg 的 `folder_id: i32`。
- **拖拽只在单项目视图下可用**（排序是项目内语义）。
- **一个任务一个会话**；`runSeq` 防止迟到回调改错状态。
