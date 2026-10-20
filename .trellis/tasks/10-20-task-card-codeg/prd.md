# PRD: 任务卡片对齐 codeg

父任务：`10-20-task-board-codeg-ui`。设计：`../10-20-task-board-codeg-ui/design.md` §3。
参考：`../10-20-task-board-codeg-ui/research/codeg-ui-details.md` §2。

## Goal

卡片要能**不打开详情就回答三件事**：这是什么任务（标题 + 哪个引擎）、它属于哪个项目多久了、
它现在需要我做什么（主按钮）。失败时错误原因直接写在卡上，不用点进去才知道。

## Requirements

### R1 首行
- 左侧引擎图标（14px）；任务没有引擎时画灰色 `Bot` 占位，保证标题左对齐。
- 中间标题 13px medium，自然换行（不用两行截断）。
- 右侧状态 chip。

### R2 元信息行
- `项目名 · 相对时间`，分隔符为浅灰 `·`。
- 项目已移除时显示「项目已移除」标签（沿用现有语义）。

### R3 错误条
- 条件：`lastError` 非空 **且** 状态为 `failed` 或 `review`。
- 样式：浅红底 + 红字 + 警告图标 + 单行截断 + 右侧「查看」按钮。
- 「查看」点击打开详情，不触发拖拽。
- `review` 且有 `resultSummary` 时，错误条下方追加两行灰色摘要。

### R4 页脚动作区
- 上方 1px 分隔线。
- **主按钮常驻可见、实心填充**（不再是描边 + hover 才显现）。
- **次按钮为圆形图标按钮，默认透明，hover / 聚焦时显现**，且出现时不改变卡片高度。
- 无动作的状态（`done` / `canceled`）不渲染页脚。

### R5 卡片容器
- `rounded-xl`，内边距 12px，白底，边框 `neutral-200`。
- hover **只改变边框颜色**为主色蓝，不加阴影、不位移。

### R6 交互不回归
- 点击卡片本体打开详情。
- 拖拽阈值、拖拽后 300ms click 抑制、在途防双击（`busy`）全部保持。
- 按钮与错误条的可点击区域不触发卡片拖拽。

## Acceptance Criteria

- [ ] **AC1** 卡片首行左侧有引擎图标；`engineId` 为空的任务显示灰色占位图标，标题不与其它卡片错位。
- [ ] **AC2** 标题可自然换行，长标题不溢出卡片。
- [ ] **AC3** 元信息行显示 `项目名 · 相对时间`，项目已移除时显示相应标签。
- [ ] **AC4** 构造一个 `failed` 且带 `lastError` 的任务，卡片内出现红色错误条，点「查看」打开详情。
- [ ] **AC5** `running` 状态不显示错误条（即使 `lastError` 残留）。
- [ ] **AC6** 主按钮常驻可见且为实心填充；次按钮默认不可见，鼠标悬停卡片后出现，且出现时卡片高度不变。
- [ ] **AC7** `done` / `canceled` 状态卡片无页脚。
- [ ] **AC8** 卡片 hover 时只变边框颜色，无阴影（`rg "shadow" src/views/tasks/components/TaskCard.vue` 无命中）。
- [ ] **AC9** 拖拽卡片到另一列仍可推进状态；按钮点击不会误触发拖拽或打开详情。
- [ ] **AC10** `vue-tsc --noEmit` + `pnpm build` 绿。

## Out of Scope

- work_branch / diff stat 展示（本项目无 worktree）
- 卡片的右键菜单 / 多选
- 状态 chip 的配色重做（沿用 `taskChip.ts`，仅在需要时补 `primaryTone`）
