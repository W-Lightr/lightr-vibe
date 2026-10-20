# PRD: 列头与看板骨架对齐 codeg

父任务：`10-20-task-board-codeg-ui`。设计：`../10-20-task-board-codeg-ui/design.md` §2。
参考：`../10-20-task-board-codeg-ui/research/codeg-ui-details.md` §1。

## Goal

看板一眼能读出四个阶段各自的「性质」：每列有识别色，空列说清「这里还没东西」而不是
显示一整块灰底；第三列的措辞准确表达「需要你处理」。

## Requirements

### R1 列头
- 3px 竖色条 + 12px 半粗标题 + 计数胶囊，同一行，高 24px。
- 竖色条颜色：`todo` 灰、`inProgress` 蓝、`attention` 琥珀、`done` 绿。
- 计数胶囊：`attention` 列且计数 > 0 时琥珀底强调，其余灰底。

### R2 列命名与空态文案
- `boardColumns.ts::columnLabel`：第三列 `attention` 由「审核」改为「**等你处理**」；
  其余保持 待办 / 进行中 / 已完成。
- `emptyColumnHint` 对齐 codeg：`还没有待办任务` / `暂无进行中的任务` /
  `暂无等你处理的任务` / `还没有已完成的任务`。

### R3 列容器
- 去掉列的灰底与边框，看板整体白底。
- 空列用虚线框表达：`rounded-xl border border-dashed`，居中提示文案。
- 拖拽合法落列时整列 `ring-2 ring-blue-400` + 浅蓝底；非法落列不变。

### R4 页面间距
- 四列之间 `gap-4`，看板四周 `p-4`。

## Acceptance Criteria

- [ ] **AC1** 四列标题依次为：待办 / 进行中 / 等你处理 / 已完成。
- [ ] **AC2** 每列标题左侧有 3px 竖色条，四列颜色互不相同且与 design §2.1 一致。
- [ ] **AC3** `attention` 列有任务时计数胶囊为琥珀色；无任务时与其它列一致。
- [ ] **AC4** 空列显示虚线框 + 对应空态文案，不再显示灰底块。
- [ ] **AC5** 把 `todo` 卡片拖到「进行中」列时整列出现蓝色 ring 高亮；拖到「已完成」列无高亮。
- [ ] **AC6** 四列文案全部来自 `boardColumns.ts`，`TaskColumn.vue` 内无硬编码列名/空态。
- [ ] **AC7** `vue-tsc --noEmit` + `pnpm build` 绿；`cargo check` 绿。
- [ ] **AC8** 拖拽、筛选、隐藏已取消/已归档开关行为零回归。

## Out of Scope

- 卡片与编辑器（子任务 2、3）
- 列表视图
- 列的折叠 / 自定义顺序 / 自定义列
