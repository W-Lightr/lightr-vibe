# PRD: 看板 UI 与拖拽

父任务：`10-17-task-board`。前置：`10-17-task-data-layer`（类型 + service + store 就绪）。
设计：`../10-17-task-board/design.md` §4/§5/§6。

## Goal

把任务变成一块能用的四列看板：一眼看出每个项目哪些活待办、哪些在跑、哪些等审核、哪些完成，
并能用鼠标拖拽推进。

## Requirements

### R1 路由与入口
- `/tasks` → `src/views/tasks/TasksView.vue`。
- `TopBar.vue` 增加「任务」按钮，`router.push('/tasks')`。

### R2 列映射（`src/views/tasks/boardColumns.ts`）
- `TaskColumnId = 'todo' | 'inProgress' | 'attention' | 'done'`。
- `STATUSES_BY_COLUMN` / `columnForStatus`（`switch` 穷举）/ `groupTasksByColumn` /
  `columnLabel`（中文：待办 / 进行中 / 审核 / 已完成）。
- `allowedDrop(from: TaskStatus, column: TaskColumnId): TaskStatus | null`：
  只返回 `todo→inProgress => 'queued'`、`review→done => 'done'`、`review→todo => 'todo'`，其余 `null`。

### R3 页面
- 顶部工具栏：项目下拉（`projectStore.projects` + 「全部项目」）、「隐藏已取消」、「隐藏已归档」、
  「新建任务」。
- 四列横向布局，每列：标题 + 数量 + 卡片列表 + 空态文案；列体 `overflow-y-auto`。
- 加载态骨架、无任务空态。
- 筛选为「全部项目」时，卡片额外显示项目名；项目已不存在的任务显示「项目已移除」。

### R4 卡片 `TaskCard.vue`
- 状态 chip（运行中主色 + 旋转图标、审核琥珀、完成绿、失败红、其余灰）。
- 标题（最多两行省略）、项目名、相对时间。
- 动作按钮按状态：
  | 状态 | 主 | 次 |
  |---|---|---|
  | todo | 开始 | — |
  | queued/preparing/running/awaiting_input | 停止 | — |
  | review | 通过 | 打回 |
  | failed | 重试 | — |
  | done/canceled | — | — |
- 点击卡片本体（非按钮）打开详情（子任务 3 提供组件；本子任务先留 emit）。

### R5 拖拽
- `useTaskDrag.ts` composable，Pointer Events + `<Teleport to="body">` 预览。
- 移动阈值 4px 才进入拖拽；同列实时重排；drop 后 `taskService.reorder` 或
  `taskService.transition`。
- 非法落列：列不显示高亮，释放后回弹，卡片抖动一次。
- 「全部项目」视图禁止拖拽（`data-drag-disabled`，卡片 `cursor-default`）。
- 拖拽结束后抑制 300ms 内的 `click`。

## Acceptance Criteria

- [ ] **AC1** `vue-tsc --noEmit` + `pnpm build` 绿；`cargo check` 不受影响仍绿。
- [ ] **AC2** `columnForStatus` 对全部 10 个 `TaskStatus` 有返回值（TS 穷举，编译即证）。
- [ ] **AC3** 四个列标题与顺序正确：待办 / 进行中 / 审核 / 已完成。
- [ ] **AC4** `todo` 卡片拖到「进行中」列 → 状态变 `queued`；拖到「审核」列 → 回弹且不发请求。
- [ ] **AC5** 同列上下拖动 → 松手后顺序保持（重读后仍一致）。
- [ ] **AC6** 项目筛选切换后列内容随之变化；「全部项目」下卡片显示项目名。
- [ ] **AC7** 隐藏已取消 / 已归档开关生效，且刷新页面后保持（localStorage）。
- [ ] **AC8** 浏览器 dev（mock）与 Tauri 下行为一致（拖拽、筛选、开关）。

## Out of Scope

- 详情抽屉、编辑器弹窗（子任务 3）
- 列表视图（codeg 有，本项目首版不做）
- 键盘操作 / 无障碍拖拽
