# Design — 设置窗口化与 Agent 管理独立滚动

> 任务：`11-10-settings-window-agent-scroll`
> 基线：Vue 3 + vue-router（`createWebHistory`）+ Tauri 2.11 / @tauri-apps/api 2.12

## 1. 目标结构

```
主窗口 label = "main"
  ├─ 任意页面点击设置入口
  │    └─ openSettingsWindow()
  │         ├─ 已有 "settings-window" → show + unminimize + setFocus
  │         └─ 无 → new WebviewWindow("settings-window", { url: "settings-window", ... })
  │
设置窗口 label = "settings-window"
  └─ 只加载 /settings-window 路由 → SettingsView（无 WorkspaceShell）
       └─ 不走编码工作区的 ACP/委派启动副作用
```

浏览器 dev（非 Tauri）：`openSettingsWindow()` 返回 `false`，调用方回退
`router.push('/settings')`，保持现有页内设置页可开发。

## 2. 窗口创建与单例（新增 `src/utils/settingsWindow.ts`）

```ts
export const SETTINGS_WINDOW_LABEL = 'settings-window'

let inFlight: Promise<boolean> | null = null

/** @returns true = 桌面端已创建/聚焦设置窗口；false = 非 Tauri，调用方回退页内路由 */
export async function openSettingsWindow(): Promise<boolean> {
  if (!isTauri()) return false
  if (inFlight) return inFlight

  inFlight = (async () => {
    const { WebviewWindow } = await import('@tauri-apps/api/webviewWindow')
    const existing = await WebviewWindow.getByLabel(SETTINGS_WINDOW_LABEL)
    if (existing) {
      await existing.unminimize().catch(() => undefined)
      await existing.show().catch(() => undefined)
      await existing.setFocus().catch(() => undefined)
      return true
    }
    const win = new WebviewWindow(SETTINGS_WINDOW_LABEL, {
      url: 'settings-window',
      title: '设置',
      width: 1100,
      height: 760,
      minWidth: 900,
      minHeight: 600,
      center: true,
      decorations: false,
      shadow: false,
    })
    await new Promise<void>((resolve, reject) => {
      void win.once('tauri://created', () => resolve())
      void win.once('tauri://error', (e) =>
        reject(new Error(String((e as { payload?: unknown }).payload ?? '窗口创建失败'))),
      )
    })
    return true
  })()

  try {
    return await inFlight
  } finally {
    inFlight = null
  }
}
```

要点：

- `inFlight` 防双击并发创建两个窗口。
- `getByLabel` 需要 `core:webview:default` 的 `allow-get-all-webviews`（已含在 `core:default`）。
- 创建权限 `core:webview:allow-create-webview-window` 加在 **main** 的 capability 上。
- 聚焦已有窗口需要 `core:window:allow-show` / `allow-set-focus` / `allow-unminimize`。
- 设置窗口自身只需要窗口三键 + 拖拽权限，不需要 shell/dialog（见 §5 拆分 capability）。

### 为什么 `url: 'settings-window'` 是安全的

- 开发：Vite 默认 `appType: 'spa'`，未知路径回退 `index.html`。
- 生产：Tauri 资源解析在找不到资源时按
  `path → path.html → path/index.html → index.html` 回退
  （`tauri-2.11.2/src/manager/mod.rs::get_asset`），因此
  `tauri://localhost/settings-window` 会拿到 `index.html`，再由 vue-router 渲染
  同名路由。窗口刷新也不会 404。
- 不使用 `router.replace` 改 URL，避免多一次导航与首屏闪烁。

## 3. 路由（`src/router/index.ts`）

新增独立顶层路由（不挂 `WorkspaceShell`）：

```ts
{
  path: '/settings-window',
  name: 'settings-window',
  // 与 /settings 同一个 SFC，按路由名区分独立窗口语义
  component: () => import('../views/settings/SettingsView.vue'),
},
```

保留现有 `/settings`（`WorkspaceShell` 子路由）作为浏览器 dev 回退。

## 4. 启动分流（`src/main.ts`）

设置窗口**不得**执行编码工作区的全局副作用：

- `useProjectStore.load()` / `useSessionStore.load()` 不需要；
- `initAcpListeners()` 会在两个 WebView 各注册一套 ACP/委派事件订阅；
- `onDelegationStarted` 的 `projectDelegation` 会 `persistMessages`，两个窗口同时写同一份
  会话数据存在后写覆盖/状态分叉风险；
- `wireTaskSessionBridge()` 与设置页无关。

因此：

```ts
import { getAppWindowLabel } from './utils/window'
import { SETTINGS_WINDOW_LABEL } from './utils/settingsWindow'

const isSettingsWindow = getAppWindowLabel() === SETTINGS_WINDOW_LABEL

async function bootstrap() {
  if (isSettingsWindow) {
    await router.isReady()
    app.mount('#app')
    return
  }
  // ... 现有 initAppData / project / session / delegation / ACP 监听 / 任务桥接 ...
}
```

`getAppWindowLabel()` 新增于 `src/utils/window.ts`：

```ts
export function getAppWindowLabel(): string | null {
  return resolved?.label ?? null
}
```

`router.isReady()` 确保懒加载的 `/settings-window` 路由解析完成后再 mount。

## 5. Tauri capabilities

### `src-tauri/capabilities/default.json`（main）

`windows` 仍为 `["main"]`，新增：

```json
"core:webview:allow-create-webview-window",
"core:window:allow-show",
"core:window:allow-set-focus",
"core:window:allow-unminimize"
```

（`getByLabel` 所需的 `core:webview:allow-get-all-webviews` 已由 `core:default` 提供。）

### 新增 `src-tauri/capabilities/settings.json`

只为设置窗口授予它实际用到的能力，避免把 shell sidecar / 目录选择权限扩散过去：

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "settings-window",
  "description": "Capability for the standalone settings window",
  "windows": ["settings-window"],
  "permissions": [
    "core:default",
    "core:window:allow-start-dragging",
    "core:window:allow-toggle-maximize",
    "core:window:allow-minimize",
    "core:window:allow-close",
    "opener:default"
  ]
}
```

> 自定义 `#[tauri::command]` 不受 capability 白名单约束（与 main 现状一致），
> 设置页的 `agent_detect` / `engine_default_set` / `delegation_*` 调用照常可用。

## 6. 设置页窗口语义（`SettingsView.vue`）

```ts
import { useRoute } from 'vue-router'
import { getAppWindow } from '../../utils/window'

const route = useRoute()
const appWindow = getAppWindow()
/** /settings-window = 独立设置窗口；/settings = 浏览器 dev / 旧页内形态 */
const isStandalone = computed(() => route.name === 'settings-window')

function goBack() {
  if (isStandalone.value && appWindow) {
    void appWindow.close()
    return
  }
  if (window.history.state?.back) router.back()
  else router.push('/')
}
```

- 返回按钮 title：独立窗口 → `关闭设置窗口`，页内 → `返回编码工作区`。
- `WindowControls` 不变：`getAppWindow()` 在设置窗口里返回当前窗口，三键天然可用。
- header 的 `data-drag-region` 不变，`App.vue` 的全局拖拽逻辑在设置窗口同样生效。

## 7. 入口统一（3 处）

| 位置 | 现状 | 改法 |
|---|---|---|
| `SidebarPanel.vue::openSettings` | `router.push('/settings')` | `openSettingsWindow()`；`false` 时回退 push |
| `HomeView.vue` 齿轮 | `router.push('/settings')` | 同上（新增 async handler + 错误 toast） |
| `useConversation.ts::passGate` | `router.push('/settings')` | `openSettingsWindow().then(handled => !handled && router.push('/settings'))` |

统一约束：新增设置入口必须调用 `openSettingsWindow()`，不得再直接 push `/settings`。

错误处理：

- `openSettingsWindow()` 抛错 → `message.error('打开设置窗口失败：<msg>')`；
- 浏览器 dev 不走错误分支（直接回退）。

## 8. Agent 管理左右独立滚动

### 8.1 外层（`SettingsView.vue`）

```html
<div class="flex min-h-0 flex-1">
  <SettingsMenu v-model:active-section="activeSection" />
  <main class="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-neutral-50">
    <AgentManageView v-if="activeSection === 'agent'" />
  </main>
</div>
```

关键：`main` 由 `overflow-y-auto` 改为 `overflow-hidden`，并成为 `flex-col` 容器，
让 `AgentManageView` 的 `h-full` 有确定高度。

### 8.2 左侧（`AgentManageView.vue`）

```html
<aside class="flex min-h-0 w-64 shrink-0 flex-col border-r border-neutral-200 bg-white">
  <div class="px-3 pb-2 pt-3 ...">Agent 列表</div>
  <div class="flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto px-2 pb-2">...</div>
  <div class="shrink-0 border-t ...">默认引擎</div>
  <div class="shrink-0 border-t ...">子代理委派</div>
</aside>
```

- `aside` 加 `min-h-0`；两个底部固定块加 `shrink-0`，列表滚动时不被压缩/滚走。

### 8.3 右侧（`AgentManageView.vue`）

把三种详情分支统一包进一个滚动容器：

```html
<div class="min-h-0 min-w-0 flex-1 overflow-y-auto">
  <OpencodeDetail v-if="showOpencodeDetail" />
  <EngineDetail
    v-else-if="selectedAgent?.enabled"
    :engine-id="selectedAgentId as EngineWire"
    :icon-svg="selectedAgent.iconSvg"
  />
  <div v-else class="flex h-full items-center justify-center text-sm text-neutral-400">
    该引擎详情即将推出
  </div>
</div>
```

- 删除 `EngineDetail` 上旧的 `overflow-y-auto` 与 `OpencodeDetail` 上的 `flex-1`，
  滚动职责只保留在容器上。
- `OpencodeDetail` / `EngineDetail` 内部布局（`p-4`、`flex-col gap-3`）不变。

## 9. ACP 适配器版本（研究结论，无代码改动）

详见 `research/acp-versions.md`。摘要：

- `claude-agent-acp` 0.103.0 起 BREAKING（仅 ACP v2、移除 v1 桥）；本项目固定
  `protocolVersion: 1`，**不能升到 0.104.0**。安全上限 `0.102.0`。
- `opencode-ai` 1.19.0 → 1.19.5 可升。
- `codex-acp` 未安装；latest 2.6.0，需要真机冒烟后再定。
- 本任务不执行全局安装命令。

## 10. 兼容性与回滚

- 回滚只需删除 `settings.json` capability、把三处入口改回 `router.push('/settings')`、
  还原布局 class；路由与 helper 可保留不生效。
- `/settings` 页内路由保留，最坏情况下仍可从浏览器 dev 使用设置页。
- 设置窗口启动分流只影响 label 为 `settings-window` 的窗口，主窗口路径不变。

## 11. 验证

1. `pnpm build`（`vue-tsc --noEmit` + `vite build`）。
2. `pnpm tauri dev` 手工验证：
   - AC1/AC2/AC3：三处入口、单例聚焦、独立形态、返回关闭；
   - AC5/AC6：窗口高度调小后左右独立滚动；
   - AC4：`pnpm dev` 浏览器里点击设置仍进入 `/settings`。
3. 若窗口创建报错，检查设置窗口 capability 是否加载
   （启动日志 / `core:webview:allow-create-webview-window`）。

## 12. 风险与取舍

| 风险 | 处理 |
|---|---|
| 设置窗口重复执行 ACP 监听/委派持久化 | main.ts 按 label 分流，跳过整套编码启动 |
| 新窗口 capability 缺失导致窗口三键/IPC 失效 | 单独 `settings.json`，权限与 main 对齐窗口控制部分 |
| 双击并发创建 | `inFlight` Promise 去重 |
| 设置窗口 URL 在打包后 404 | 依赖 Tauri `index.html` fallback（已在源码确认）+ Vite SPA fallback |
| 两套设置页形态分叉 | 同一个 `SettingsView.vue`，仅按路由名区分返回语义 |
