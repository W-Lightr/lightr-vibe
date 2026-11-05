# 编码入口性能优化 — 执行计划

## 前置

- 任务状态进入 `in_progress`（`task.py start`）后才开始改代码。
- 改动前记录基线：`dist/assets` 体积清单与 `dist/index.html` 预加载列表（已有基线见 `research/entry-loading-evidence.md`）。
- 无测试框架；验证以 `pnpm build` + `pnpm dev:tauri` 手动流程为准。

## 实施清单

### 1. 重模块 loader 与预取

- [x] 新增 `src/views/coding/lazyWorkspace.ts`
  - `loadFileEditor()` / `loadTerminal()`
  - `prefetchFileEditor()` / `prefetchTerminal()`（模块级 Promise 去重，失败静默）
  - `scheduleIdle(task, timeoutMs)`（`requestIdleCallback` + `setTimeout` 兜底）
- [x] 复核 `noUnusedLocals` / `strict`，不引入 `any` 或非空断言。

### 2. Loading / 错误组件

- [x] 新增 `src/views/coding/components/EditorLoading.vue`（白底 + spinner + 「加载编辑器…」）
- [x] 新增 `src/views/coding/components/TerminalLoading.vue`（`bg-neutral-900` + 「加载终端…」）
- [x] 新增 `src/views/coding/components/AsyncPanelError.vue`（展示错误 + 重试按钮）

### 3. Monaco worker 迁移

- [x] 新增 `src/views/coding/monacoSetup.ts`，迁入 `main.ts:29-33` 的 5 个 `?worker` 导入与 `MonacoEnvironment` 赋值。
- [x] `FileEditorView.vue` 第一行 import `../monacoSetup`，确认在 `monaco-editor-vue` 之前。
- [x] `DiffViewer.vue` 同样首行 import（实现时发现的额外 Monaco 入口）。
- [x] 删除 `main.ts` 中的 worker 导入与 `MonacoEnvironment` 赋值。

### 4. xterm CSS 迁移

- [x] 从 `main.ts` 删除 `@xterm/xterm/css/xterm.css` 导入。
- [x] 在 `TerminalPanel.vue` 顶部导入该 CSS。

### 5. 工作区异步化

- [x] `CodingWorkspace.vue` 用 `defineAsyncComponent` 替换 `FileEditorView` / `TerminalPanel` 静态导入。
- [x] 配置 `loader`、`loadingComponent`、`errorComponent`、`delay: 0`、`timeout: 30_000`。
- [x] 终端抽屉外层补 `bg-neutral-900`。
- [x] `onMounted` 中 `scheduleIdle(prefetchFileEditor, 3000)`；与现有 resize 监听清理互不影响。
- [x] `RightPanel.vue` 中的 `DiffViewer` 同样异步化（否则 Monaco 仍会回到入口静态依赖）。

### 6. 预取触发点

- [x] `TopBar.vue` 终端按钮加 `@mouseenter` / `@focus` 调 `prefetchTerminal()`（保留原 `toggle-terminal` emit）。
- [x] `FilesTree.vue` 文件树滚动容器加 `@pointerenter` 调 `prefetchFileEditor()`。
- [x] `HomeView.vue` 的 `features` 增加可选 `prefetch`；编码项用 `.then((m) => m.default)` 预热外壳；卡片绑定 `@pointerenter` / `@focus`。

### 7. 后续功能约定文档

- [x] 新增 `.trellis/spec/frontend/lazy-loading.md`（路由懒加载、重依赖异步、预取、loading、Monaco setup 规则）。
- [x] 更新 `.trellis/spec/frontend/index.md` 索引。
- [x] 更新 `.trellis/spec/frontend/component-guidelines.md` 的 Monaco worker 配置位置（`main.ts` → `views/coding/monacoSetup.ts`）。

### 8. 验证

- [x] `pnpm build` 通过（check 阶段复跑，EXITCODE=0）。
- [x] 检查 `dist/index.html` 与 `dist/assets`：
  - 入口不包含 `editor.api` 静态/preload 引用。
  - 已记录拆分后 `CodingWorkspace`、`FileEditorView`/Monaco、`TerminalPanel`/xterm 的 chunk 体积（见 `research/entry-loading-evidence.md`）。
- [ ] `pnpm dev:tauri` 手动验证（待用户/主会话执行）：
  - [ ] 点击「编码」立即进入，首帧不等待编辑器。
  - [ ] 首次打开文件出现 loading，随后可编辑；Ctrl+S 保存、dirty、冲突提示正常。
  - [ ] 首次打开终端出现 loading，随后占位终端可用；开合正常。
  - [ ] 会话切换、侧栏、顶栏、右栏文件树正常。
  - [ ] 控制台无 worker / chunk / Vue 警告。
- [ ] dev 模式记录首次进入 `/coding` 的观察耗时，写入任务备注与最终报告。

## Check 结果（2026-11-05）

- 结论：PASS after fixes。
- 关键修复：路由与首页预热必须用 `.then((m) => m.default)`，否则打包器命名空间包装会把 `editor.api` 重新拉回入口静态依赖（已在 `src/router/index.ts`、`src/views/HomeView.vue` 加注释并写入 spec）。
- 额外修复：`DiffViewer`/`RightPanel` 异步化（实现时发现）；`HomeView` 预热补 `.catch`；spec 中 Monaco worker 时机描述校正。
- 改后体积：`/coding` 路由 chunk 约 140 KB（静态闭包约 705.6 KB），入口 preload 无 `editor.api` / xterm。
- 剩余：`pnpm dev:tauri` 交互式验证与 dev 首进耗时对比。

## 风险文件与回滚点

| 文件 | 风险 | 回滚 |
|---|---|---|
| `src/main.ts` | 删除 worker/xterm CSS 后遗漏配置 | 恢复导入块 |
| `src/views/coding/CodingWorkspace.vue` | 异步组件配置错误导致编辑器/终端不渲染 | 恢复静态 import |
| `src/views/coding/components/FileEditorView.vue` | `monacoSetup` 导入顺序错误 | 调整为首行导入 |
| `src/views/coding/components/TerminalPanel.vue` | CSS 迁移后样式缺失 | 恢复 `main.ts` 全局导入 |
| `src/views/HomeView.vue` | 预取写错导致首页报错 | `prefetch` 为可选字段，删除即可 |
| `src/router/index.ts` | 把 `.then((m) => m.default)` 简化掉会让 Monaco 回到入口 | 恢复 `.then((m) => m.default)` |

建议分两次提交：①异步拆分 + worker/CSS 迁移（可独立回滚）；②预取 + 约定文档。

## Review Gates

1. 实施完成、`pnpm build` 通过后，先自查 diff 是否只触及本任务文件。
2. 手动验证清单全部通过后，再进入 `trellis-check`。
3. check 通过后更新 spec（若实施阶段未完成）并提交。
