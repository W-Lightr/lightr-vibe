# 编码入口加载证据

## 测量环境

- 生产构建产物：`dist/`（改前基线，`pnpm build` 产物）
- 体积统计：`dist/assets` 文件大小
- 依赖链：源码静态 import 检查 + 构建 chunk 内容检查

## 改前基线（dist/assets）

| 文件 | 体积 |
|---|---|
| `editor.api-BE4p7nyL.js`（Monaco 核心） | 2673.0 KB |
| `CodingWorkspace-ClvvCZVn.js` | 1738.0 KB |
| `CodingWorkspace-BFSGQXED.css` | 88.1 KB |
| `editor-DjpsE8to.css` | 96.8 KB |
| `index-CTKl59NL.js`（入口） | 105.3 KB |

验证：`CodingWorkspace-ClvvCZVn.js` 首行包含 `from"./editor.api-BE4p7nyL.js"`，说明 Monaco 核心被 `/coding` 路由 chunk 静态拉入。

## 静态引用链

```
src/router/index.ts
  └─ () => import('../views/coding/CodingWorkspace.vue')
       ├─ import FileEditorView from './components/FileEditorView.vue'   // CodingWorkspace.vue:9
       │    └─ import MonacoEditor from 'monaco-editor-vue'             // FileEditorView.vue:7
       │         └─ import * as monaco from 'monaco-editor'             // monaco-editor-vue/dist/index.js
       └─ import TerminalPanel from './components/TerminalPanel.vue'     // CodingWorkspace.vue:11
            └─ import { Terminal } from '@xterm/xterm'                  // TerminalPanel.vue:3
```

这两块首次进入并不渲染：

- `FileEditorView` 仅在 `activeTab.kind === 'file'` 时渲染（`CodingWorkspace.vue:202`）。
- `TerminalPanel` 仅在 `terminalVisible` 为真时渲染（`CodingWorkspace.vue:196`）。

## 其它已确认成本

- `src/main.ts:29-33` 启动时静态导入 5 个 Monaco `?worker` 入口；worker 实例是懒创建，但模块解析在启动阶段发生。
- `src/main.ts` 还全局导入 `@xterm/xterm/css/xterm.css`（7112 B）。
- `vite.config.ts` 设置 `optimizeDeps.exclude: ['monaco-editor']`，dev 下 Monaco 按源码模块加载/转换。
- `NewSessionDialog.vue:40-42` 在 `onMounted` 调 `engineStore.load()`；该弹窗在 SidebarPanel、TopBar、CodingWorkspace 中常驻挂载。
- `SidebarPanel` 全量遍历/排序/分组会话，无虚拟滚动（本任务不处理）。

## 结论

入口卡顿的主因是 `/coding` chunk 静态包含 Monaco（2.67 MB）与 xterm，而不是路由守卫或业务请求。把这两块改为按需加载即可把主因从入口路径移除。

---

## 改后实测（check 阶段，`pnpm build` 产物）

| 文件 | 体积 |
|---|---|
| `CodingWorkspace-*.js`（`/coding` 路由 chunk） | 140.0 KB |
| `dist-*.js`（monaco-editor-vue + monaco 主包） | 1270.4 KB |
| `editor.api-*.js`（Monaco 核心） | 2673.0 KB |
| `FileEditorView-*.js` | 4.3 KB |
| `DiffViewer-*.js` | 0.9 KB |
| `TerminalPanel-*.js` / `TerminalPanel-*.css` | 325.3 KB / 3.8 KB |
| `CodingWorkspace-*.css` | 3.2 KB |
| `index-*.js`（入口） | 105.1 KB |

静态 import 图实测（解析每个 chunk 的 `from"./..."`，忽略 `__vite__mapDeps` 里的动态预加载列表）：

| 链路 | 文件数 | JS 合计 | Monaco / xterm |
|---|---|---|---|
| 入口（`index.html` script + modulepreload） | 13 | 373.5 KB | 无 |
| `/coding` 路由 chunk 静态闭包 | 32 | 705.6 KB（相对入口增量 332.1 KB） | 无 |
| 编辑器 chunk 闭包（打开文件 / 空闲预取） | 37 | 4653.8 KB | `editor.api` 2673.0 + `dist` 1270.4 |
| 变更页 chunk 闭包（切到「变更」） | 5 | 4011.3 KB | 同上（DiffViewer 自身仅 0.9 KB） |
| 终端 chunk 闭包（悬停/打开终端） | 2 | 391.0 KB | xterm |

对比改前（`CodingWorkspace-ClvvCZVn.js` 1738.0 KB，且静态拉入 `editor.api` 2673.0 KB，合计 ≈ 4411 KB）：
`/coding` 首屏 JS 由 ≈ 4411 KB 降到 ≈ 332 KB（-92%），CSS 由 184.9 KB（88.1 + 96.8）降到 3.2 KB，xterm CSS（7.1 KB）也不再进入口。

### 关键约束：懒加载 SFC 必须显式 `.then((m) => m.default)`

若写成 `() => import('...CodingWorkspace.vue')`（返回整段模块命名空间），打包器会为该 chunk 生成命名空间包装
（产物里形如 `pl = e({ default: () => ml })`），而「把对象包装成 ESM 命名空间」的共享 helper 目前被打进 Monaco chunk，
于是路由 chunk 出现 `import{r as e} from "./dist-*.js"` 的**静态**边，`editor.api` 又被拉回首屏。
`HomeView` 的预热 import 同理。check 阶段实测：两处任一改回裸 `import()` 都会复现该静态边（两处都改回时路由 chunk 静态闭包重新包含 `editor.api` + `dist`）。
因此 `src/router/index.ts` 与 `HomeView.vue` 保留 `.then((m) => m.default)`，并已写入 `spec/frontend/lazy-loading.md`。

