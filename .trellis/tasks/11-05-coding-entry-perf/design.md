# 编码入口性能优化 — 技术设计

## 1. 目标与边界

把 Monaco 与 xterm 从 `/coding` 首屏加载路径中移出，在用户真正需要文件编辑器/终端时再加载；进入工作区后空闲预取，首次打开尽量无感。

边界：

- 只改编码工作区与其入口预取；不改会话/ACP/任务业务逻辑。
- 不引入新的全局状态、新依赖或构建插件。
- 不改 `optimizeDeps.exclude`，不引入 `manualChunks` 全量重构。

## 2. 现状与目标形态

现状（改前）：

```
Home → click → /coding chunk (1.74MB)
                  ├─ Monaco editor.api (2.67MB)   ← 静态依赖，首屏加载
                  ├─ xterm                         ← 静态依赖，首屏加载
                  └─ 工作区组件
```

目标：

```
Home → hover 预取 → /coding shell chunk（无 Monaco/xterm）
                  ├─ 首屏：Sidebar + TopBar + Conversation + Composer
                  ├─ idle 预取：FileEditorView chunk（含 Monaco）
                  ├─ hover 文件树 / 终端按钮：提前 import 对应 chunk
                  └─ 用户打开文件/终端：异步组件 + loading
```

## 3. 模块设计

### 3.1 `src/views/coding/lazyWorkspace.ts`（新增，feature-local 纯模块）

职责：集中管理重模块的 loader 与预取，避免散落在组件里。

```ts
// 导出
export function loadFileEditor(): Promise<typeof import('./components/FileEditorView.vue')>
export function loadTerminal(): Promise<typeof import('./components/TerminalPanel.vue')>

export function prefetchFileEditor(): void
export function prefetchTerminal(): void
export function scheduleIdle(task: () => void, timeoutMs?: number): void
```

实现要点：

- `prefetch*` 用模块级 `Promise | null` 去重；动态 `import()` 自身也有缓存，去重主要避免重复调度。
- `scheduleIdle` 优先 `window.requestIdleCallback`（`{ timeout }`），不可用时 `setTimeout(task, 200)`。
- 预取失败静默吞掉（本地 chunk 缺失时不应产生未处理 rejection），真正渲染时由异步组件的 error fallback 呈现。
- 不引入 Pinia/组件状态；纯模块级缓存，符合目录规范中「feature-local plain module」定位。

### 3.2 异步组件（`CodingWorkspace.vue`）

```ts
const FileEditorView = defineAsyncComponent({
  loader: loadFileEditor,
  loadingComponent: EditorLoading,
  errorComponent: AsyncPanelError,
  delay: 0,
  timeout: 30_000,
})
const TerminalPanel = defineAsyncComponent({
  loader: loadTerminal,
  loadingComponent: TerminalLoading,
  errorComponent: AsyncPanelError,
  delay: 0,
  timeout: 30_000,
})
```

约束：

- 不使用 `Suspense` 包裹工作区；异步边界只影响编辑器/终端子树。
- `SidebarPanel`、`ConversationView`、`MessageComposer`、`TopBar` 保持同步导入（首屏需要）。
- 终端容器（`CodingWorkspace.vue` 终端抽屉外层）补 `bg-neutral-900`，避免 loading 白底与终端深色之间闪烁。

### 3.3 预取触发点

| 触发点 | 预取目标 | 时机 |
|---|---|---|
| `CodingWorkspace` `onMounted` | FileEditorView | `scheduleIdle(..., 3000)`，首帧之后 |
| `FilesTree` 滚动容器 `pointerenter` | FileEditorView | 用户开始浏览文件 |
| `TopBar` 终端按钮 `mouseenter` / `focus` | TerminalPanel | 用户准备开终端 |
| `HomeView` 功能卡 `pointerenter` / `focus` | `/coding` shell chunk | 点击前预热路由（不含 Monaco/xterm） |

`HomeView.features` 增加可选 `prefetch?: () => void`，编码项填 `() => { void import('../views/coding/CodingWorkspace.vue') }`。后续写作功能沿用同一字段。

### 3.4 Monaco worker 初始化迁移

新增 `src/views/coding/monacoSetup.ts`：

- 迁入 `src/main.ts:29-33` 的 5 个 `?worker` 导入与 `MonacoEnvironment` 赋值。
- `FileEditorView.vue` 中**第一行 import** `../monacoSetup`，再 import `monaco-editor-vue`，保证 Monaco 模块求值前 worker 配置已就位。
- `main.ts` 删除 worker 导入与 `MonacoEnvironment` 赋值。
- 当前只有 `FileEditorView` 使用 `monaco-editor`，因此迁移不会影响其它入口；后续若有新入口使用 Monaco，必须同样先导入 `monacoSetup`（写入约定文档）。

### 3.5 xterm CSS 迁移

- 从 `main.ts` 移除 `import '@xterm/xterm/css/xterm.css'`。
- 在 `TerminalPanel.vue` 顶部导入同一 CSS，随终端 chunk 一起按需加载。
- 该 CSS 仅服务 xterm DOM，迁移无全局样式依赖。

## 4. Loading / 错误反馈

新增 3 个小组件（`src/views/coding/components/`）：

- `EditorLoading.vue`：白底、居中 spinner + 「加载编辑器…」，占满编辑器区域。
- `TerminalLoading.vue`：`bg-neutral-900`、浅色文字 + 「加载终端…」。
- `AsyncPanelError.vue`：接收 `defineAsyncComponent` 注入的 `error` / `retry`，展示「加载失败，点击重试」。

`delay: 0` 保证本地 chunk 读取期间立即可见反馈；不使用骨架屏库。

## 5. 后续功能约定（交付物）

新增 `.trellis/spec/frontend/lazy-loading.md` 并加入 `frontend/index.md` 索引，内容至少包含：

1. 功能入口必须是路由级 `() => import(...)`，首页/共享布局不得静态导入功能页。
2. Monaco、xterm、画布等重依赖只能出现在功能内的异步组件中。
3. 异步组件必须提供 loading/error fallback，不得用全局 `Suspense` 阻塞整页。
4. 预取只做「空闲/悬停」，不得在首页同步加载重依赖。
5. 新增使用 Monaco 的模块必须先导入 `monacoSetup`。

同时更新 `.trellis/spec/frontend/component-guidelines.md` 中 Monaco worker 配置位置说明（`main.ts` → `views/coding/monacoSetup.ts`）。

## 6. 兼容性与风险

| 风险 | 影响 | 缓解 |
|---|---|---|
| worker 配置晚于 Monaco 求值 | 编辑器报 "Could not create web worker" | `monacoSetup` 在 `FileEditorView` 第一行导入；构建后手动打开文件验证 |
| 预取在低端机造成 idle 卡顿 | 交互掉帧 | 只在 idle + timeout 后预取编辑器；终端只悬停/点击时预取 |
| 终端 CSS 延迟加载 | 首次终端短暂样式缺失 | `TerminalLoading` 使用 Tailwind 样式，不依赖 xterm CSS |
| 异步 chunk 加载失败 | 编辑器/终端空白 | `errorComponent` 提供重试 |
| `requestIdleCallback` 类型/兼容 | TS 构建失败或运行时报错 | 特性检测 + `setTimeout` 兜底；TS `lib.dom` 已含该 API |
| Vite 仍把 Monaco 合入入口 | 优化无效 | 构建后检查 `dist/index.html` 与 chunk 引用，确认 `editor.api` 不在入口 preload/静态依赖 |

## 7. 回滚

- 主要回滚点：恢复 `CodingWorkspace.vue` 的静态 import、`main.ts` 的 worker/xterm CSS 导入，删除 `lazyWorkspace.ts` 与 loading 组件。
- 分步提交建议：先落地异步拆分（可独立回滚），再落地预取/约定文档。
- 不涉及数据迁移或持久化格式变更，回滚无数据风险。

## 8. 验证口径

- `pnpm build` 通过（`vue-tsc --noEmit` + `vite build`）。
- 对比 `dist/assets` 体积与 `dist/index.html` 预加载列表：入口不再包含 `editor.api` / `CodingWorkspace` 中的 Monaco/xterm。
- `pnpm dev:tauri` 手动验证：入口点击、打开文件、保存、冲突提示、终端开关、会话切换、无控制台报错。
- dev 模式记录首次进入 `/coding` 的观察耗时（Performance/Network），与改前基线对比。
