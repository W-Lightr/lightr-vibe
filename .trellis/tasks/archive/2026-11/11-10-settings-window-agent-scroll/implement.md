# Implement — 设置窗口化与 Agent 管理独立滚动

> 执行前置：`task.py start` 之后才动代码。
> 本任务不需要子代理，`implement.jsonl` / `check.jsonl` 保持为空即可。

## 0. 基线检查

- [ ] `git status` 干净（仅本任务目录未跟踪）。
- [ ] 确认当前分支：`codex/subagent-delegation`；如需独立分支再切（不强制）。

## 1. 窗口工具

- [ ] `src/utils/window.ts`：新增 `getAppWindowLabel(): string | null`，返回 `resolved?.label ?? null`。
- [ ] 新建 `src/utils/settingsWindow.ts`：
  - `SETTINGS_WINDOW_LABEL = 'settings-window'`
  - `openSettingsWindow(): Promise<boolean>`：`isTauri()` 守卫、`inFlight` 去重、
    动态导入 `@tauri-apps/api/webviewWindow`、`getByLabel` 复用/聚焦、
    `new WebviewWindow(...)` 创建、`tauri://created` / `tauri://error` Promise 包装。
  - 窗口参数：`url: 'settings-window'`、`title: '设置'`、`1100x760`、
    `minWidth: 900` / `minHeight: 600`、`center: true`、`decorations: false`、`shadow: false`。

## 2. 路由与启动分流

- [ ] `src/router/index.ts`：新增顶层路由
      `{ path: '/settings-window', name: 'settings-window', component: () => import('../views/settings/SettingsView.vue') }`。
      保留 `/settings`（WorkspaceShell 子路由）。
- [ ] `src/main.ts`：
  - 引入 `getAppWindowLabel` / `SETTINGS_WINDOW_LABEL`；
  - `const isSettingsWindow = getAppWindowLabel() === SETTINGS_WINDOW_LABEL`；
  - `bootstrap()` 最前面分支：`await router.isReady(); app.mount('#app'); return`；
  - 主窗口路径保持原样（不要重排现有逻辑）。

## 3. 设置页窗口语义与布局

- [ ] `src/views/settings/SettingsView.vue`：
  - 引入 `useRoute`、`getAppWindow`；
  - `isStandalone = route.name === 'settings-window'`；
  - `goBack()`：独立窗口且 `appWindow` 存在 → `appWindow.close()`；否则保留原返回逻辑；
  - 返回按钮 title 按 `isStandalone` 切换；
  - `<main>`：`overflow-y-auto` → `flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-neutral-50`。
- [ ] `src/views/settings/components/AgentManageView.vue`：
  - `aside` 加 `min-h-0`；
  - 「默认引擎」「子代理委派」两个底部块加 `shrink-0`；
  - 右列三种分支统一包进 `<div class="min-h-0 min-w-0 flex-1 overflow-y-auto">`；
  - 删除 `OpencodeDetail` 的 `flex-1`、`EngineDetail` 的 `overflow-y-auto`。

## 4. 设置入口统一

- [ ] `src/views/coding/components/SidebarPanel.vue::openSettings`：
      `async`；先 `await openSettingsWindow()`，返回 `true` 直接结束；
      `false` 时保留 `if (!isSettingsRoute.value) void router.push('/settings')`；
      捕获异常 → `message.error('打开设置窗口失败：...')`。
- [ ] `src/views/HomeView.vue`：齿轮按钮改为 async handler，逻辑同上
      （`useMessage` 已可由全局 provider 提供）。
- [ ] `src/composables/useConversation.ts::passGate`：
      `void openSettingsWindow().then(handled => { if (!handled) void router.push('/settings') }).catch(() => { void router.push('/settings') })`。
- [ ] 全仓 `rg "router.push\('/settings'\)" src`：只允许出现在上述 fallback 分支。

## 5. Tauri capabilities

- [ ] `src-tauri/capabilities/default.json`（main）新增：
      `core:webview:allow-create-webview-window`、
      `core:window:allow-show`、`core:window:allow-set-focus`、`core:window:allow-unminimize`。
- [ ] 新建 `src-tauri/capabilities/settings.json`：
      `windows: ["settings-window"]`，权限 = `core:default` +
      `start-dragging` / `toggle-maximize` / `minimize` / `close` + `opener:default`。
- [ ] 不改 Rust 源码；不改 ACP 协议实现。

## 6. 自动验证

- [ ] `pnpm build` 通过（vue-tsc + vite build）。
- [ ] `git diff --check` 无空白错误。
- [ ] 如环境允许：`cd src-tauri && cargo check`（仅确认 capability JSON 未影响构建；
      本任务不改 Rust，可跳过）。

## 7. 运行时手工验证（`pnpm tauri dev`）

- [ ] 侧栏底部齿轮 → 独立设置窗口，主窗口停在原页面。
- [ ] 首页齿轮 → 同上。
- [ ] 在编码页让引擎门禁失败（或临时构造）→ `passGate` 打开设置窗口。
- [ ] 设置窗口已开时重复点击 → 只聚焦，不新增窗口（任务栏窗口数不变）。
- [ ] 设置窗口无主应用侧栏；顶部可拖动；最小化/最大化/关闭可用；返回按钮关闭窗口。
- [ ] 缩小窗口高度：
  - 左列 Agent 列表出现独立滚动条；「默认引擎」「子代理委派」仍可见；
  - 右列详情出现独立滚动条；外层页面无整页滚动条。
- [ ] 浏览器 dev（`pnpm dev`）：点击设置进入 `/settings`，无控制台错误。
- [ ] 设置窗口内 Agent 检测/默认引擎/委派开关仍可保存。

## 8. 规格更新与提交（Phase 3）

- [ ] 更新 `.trellis/spec/frontend/tauri-bridge.md`：
  - 新增「设置窗口（settings-window）」小节：label、capability、启动分流、
    `openSettingsWindow()` 单一入口、设置页独立滚动布局约定；
  - 修正「设置入口只放在侧栏底部」的旧描述：入口仍集中，但动作改为开窗口。
- [ ] `python ./.trellis/scripts/task.py add-context ...` 可选；至少保留
      `research/acp-versions.md`。
- [ ] 提交：`feat(settings): 独立设置窗口与 Agent 管理独立滚动`。
- [ ] `python ./.trellis/scripts/task.py finish` + `archive`（按团队流程）。

## 回滚点

| 阶段 | 回滚方式 |
|---|---|
| 窗口工具/路由/入口 | 还原三处入口为 `router.push('/settings')`，可保留 helper 与路由不引用 |
| 启动分流 | 删除 `main.ts` 的 `isSettingsWindow` 分支 |
| capability | 删除 `settings.json`，移除 default.json 新增四项 |
| 布局 | 还原 `SettingsView` / `AgentManageView` 的 class |

## 禁止事项

- 不要把设置窗口挂到 `WorkspaceShell`。
- 不要在设置窗口执行 `initAcpListeners()` / `projectDelegation()` / `wireTaskSessionBridge()`。
- 不要在组件里直接 `new WebviewWindow`（统一走 `openSettingsWindow()`）。
- 不要执行 `npm i -g` 改动用户全局适配器（版本结论仅写入 research）。
