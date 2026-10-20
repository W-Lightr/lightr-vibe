# Implement: 任务看板对齐 codeg UI（父任务）

顺序：子任务 1（列）→ 2（卡片）→ 3（编辑器）。每个子任务结束时 `pnpm build` 全绿并人工比对一次截图。

## 验证命令

```powershell
pnpm vue-tsc --noEmit
pnpm build
cargo check                 # workdir: src-tauri（本轮不应有 Rust 改动，仅确认无回归）
```

## 0 · 准备

- [ ] 读 `research/codeg-ui-details.md`，对照 codeg 行号确认每处细节
- [ ] 确认 `src/utils/engineIcons.ts` 的 `engineIcon()` 可直接用于卡片与下拉

## 1 · 子任务 `10-20-board-chrome`

- [ ] `boardColumns.ts`：`columnLabel` 第三列改「等你处理」；`emptyColumnHint` 四条文案对齐 codeg
- [ ] `TaskColumn.vue`：
  - [ ] 列头：3px 竖色条 + `text-xs font-semibold` 标题 + 计数胶囊（attention 且有内容时琥珀）
  - [ ] 去掉列背景与边框，改白底
  - [ ] 空态改虚线框 `rounded-xl border border-dashed border-neutral-300`
  - [ ] 拖拽高亮改 `ring-2 ring-blue-400` + `bg-blue-50/40`
- [ ] `TasksView.vue`：`<main>` 改 `gap-4 p-4`

**回滚点**：`git checkout src/views/tasks/boardColumns.ts src/views/tasks/components/TaskColumn.vue src/views/tasks/TasksView.vue`

## 2 · 子任务 `10-20-task-card-codeg`

- [ ] 新增 `components/EngineMark.vue`：`engineIcon(engineId)` 有则渲染，无则灰 `Bot` 占位
- [ ] `TaskCard.vue` 重做结构：
  - [ ] 外层 `rounded-xl p-3`，去掉 hover 阴影，hover 变 `border-blue-400`
  - [ ] 首行：EngineMark + 标题（`break-words`，去 `line-clamp-2`）+ chip
  - [ ] 元信息行：项目名 `·` 相对时间，分隔符 `text-neutral-300`
  - [ ] 错误条：`failed` / `review` 且有 `lastError` 时红色条 + 「查看」
  - [ ] `review` 且有 `resultSummary` 时追加两行摘要
  - [ ] 页脚：`border-t border-neutral-100 pt-3`，实心黑主按钮常驻 + 圆形图标次按钮 hover 显现
- [ ] `taskChip.ts`：新增 `primaryTone(status)`，卡片与详情共用
- [ ] `useTaskDrag` / `shouldSuppressClick` 行为不变（回归检查）

**回滚点**：`git checkout src/views/tasks/components/TaskCard.vue src/views/tasks/components/taskChip.ts`，删除 `EngineMark.vue`

## 3 · 子任务 `10-20-task-editor-codeg`

- [ ] `TaskEditorDialog.vue`：
  - [ ] 标题：无边框 `text-lg font-medium`，去 label
  - [ ] 引擎 pill 下拉：`rounded-full` + 图标 + `ChevronDown`；展开面板样式对齐
  - [ ] 提示词框：`min-h-[6rem] max-h-[14rem]`，去边框内的多余装饰
  - [ ] 「目标」区：小号大写 label + 文件夹下拉 + 一行说明
  - [ ] 页脚：取消 / 保存（实心黑），不渲染模板按钮
- [ ] 校验错误文案显示在页脚上方

**回滚点**：`git checkout src/views/tasks/components/TaskEditorDialog.vue`

---

## 关键约束

- **只改视觉与结构，不改数据层**：`types/task.ts`、`services/taskService.ts`、`stores/task.ts`、
  `src-tauri/` 一律不动。
- **色板换算**见 `design.md` §1，不得引入 shadcn token 或新依赖。
- **中文文案**，列名/空态只从 `boardColumns.ts` 取。
- 组件不直接 `invoke` / 调 service。
- PowerShell `Get-Content`/`Set-Content` 会破坏本仓库 UTF-8 中文，改用 edit/write 工具或 Node/Python。

## 人工验收（对照用户提供的两张截图）

- [ ] 列头有竖色条，第三列显示「等你处理」
- [ ] 空列是虚线框，不是灰块
- [ ] 卡片主按钮常驻可见且为实心；次按钮 hover 才出现
- [ ] 失败卡片内有红色错误条 + 「查看」
- [ ] 编辑器引擎为带图标的下拉列表
