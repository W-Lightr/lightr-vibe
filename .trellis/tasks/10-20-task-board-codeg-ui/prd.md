# 任务看板对齐 codeg UI（父任务）

用户提供的两张参考截图 = codeg 的任务看板与新建任务对话框。
研究：`research/codeg-ui-details.md`（含 codeg 行号与逐项差异）。

## Goal / 用户价值

上一轮任务看板（`archive/2026-10/10-17-task-board`）功能完整，但视觉是「自创的灰底看板 +
描边卡片」，与用户期望的 codeg 观感差距明显：列没有识别色、空列是灰块而不是虚线区、
卡片主按钮要 hover 才出现、错误信息只在详情里可见、编辑器引擎选择不符合预期。

本轮**只做视觉与交互观感的对齐**，不新增业务能力。

## 范围（用户已确认）

四项一起做：

- **D1 列头与看板骨架**：3px 竖色条 + 计数胶囊；列体白底无边框，空列用虚线框；第三列更名。
- **D2 任务卡片**：图标 + 标题 + chip 单行；元信息行带 `/` `·` 分隔；分隔线页脚；
  实心主按钮常驻 + 圆形图标次按钮 hover 显现；失败/待审核卡片内红色错误条。
- **D3 编辑器**：无边框大标题输入；引擎**下拉列表**（带图标）；大提示词框；
  「目标」区（文件夹下拉）；页脚「取消 / 保存」。
- **D4 页面级细节**：hover 只变边框色不变阴影；卡片圆角与内边距对齐 codeg 比例。

**明确不做**（沿用父任务已归档的差距清单）：worktree / 分支选择、合并、PR、preflight、
agent 图标之外的 work_branch 与 diff stat。

> 修订记录：codeg 页脚还有一个「模板」按钮，背后需要模板存取的数据层。
> 用户确认**模板本轮不做**，该按钮一并移除（不放禁用占位，避免死 UI）。

## 子任务映射

| # | 子任务 | 目录 | 交付 |
|---|---|---|---|
| 1 | 列头与看板骨架 | `10-20-board-chrome` | 列头、空态、列命名、看板背景 |
| 2 | 任务卡片 | `10-20-task-card-codeg` | 卡片全部结构重做 |
| 3 | 新建/编辑对话框 | `10-20-task-editor-codeg` | 编辑器布局 + 引擎下拉 |

顺序：1 → 2 → 3（3 的编辑器依赖 2 定下的卡片/表单视觉语言）。

## 跨子任务 Acceptance Criteria

- [ ] **PAC1** 颜色只用项目既有色板：`neutral-*` 灰阶 + `blue-500/600` 主色 + 语义色
      `red-*` / `amber-*` / `green-*`（`.trellis/spec/frontend/ui-style.md`）。不引入 shadcn token。
- [ ] **PAC2** 不新增依赖；图标一律 `lucide-vue-next`，引擎图标走 `src/utils/engineIcons.ts`。
- [ ] **PAC3** 中文文案；列名与空态文案统一由 `boardColumns.ts` 提供，组件不硬编码。
- [ ] **PAC4** 拖拽、双击保护、筛选、localStorage 开关等既有交互零回归。
- [ ] **PAC5** 浏览器 dev（mock）与 Tauri 下视觉一致（纯 CSS 改动，不得出现 `isTauri()` 分支影响样式）。
- [ ] **PAC6** `vue-tsc --noEmit` + `pnpm build` 全绿；`cargo check` 不受影响。
- [ ] **PAC7** 三列卡片的可点击区域与按钮可点击区域不互相吞掉（卡片点开详情、按钮执行动作）。

## Out of Scope

- 深色模式（本项目 light only）
- 列表视图（codeg 有 `task-list.tsx`，本轮不做）
- worktree / 分支 / 合并 / diff stat 展示
- 键盘无障碍拖拽（codeg 有 `role="button"` + Enter/Space，本轮可选）

## 验收方式

人工在 `pnpm dev:tauri` 下与用户提供的两张截图逐项比对，按各子任务 PRD 的
Acceptance Criteria 逐条检查；`pnpm build` 作为机器门禁。
