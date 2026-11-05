# 编码入口性能优化

## Goal

点击首页「编码」后应尽快看到编码工作区，而不是等待数 MB 的编辑器/终端依赖下载、解析、执行完毕。同时为后续「写作」等功能建立可复用的懒加载约定，避免每个新功能入口重复制造同样的卡顿。

用户价值：入口点击即时反馈；首次打开文件/终端时的延迟可控且可感知（loading/预取），而不是整页无响应。

## Background（已确认事实）

- `/coding` 路由本身已是懒加载（`src/router/index.ts`），但 `CodingWorkspace.vue` 静态导入了所有工作区组件。
- 静态引用链导致入口即加载 Monaco 与 xterm：
  - `src/views/coding/CodingWorkspace.vue:9` → `FileEditorView.vue:7`（`monaco-editor-vue` → `monaco-editor`）
  - `src/views/coding/CodingWorkspace.vue:11` → `TerminalPanel.vue:3-4`（`@xterm/xterm`）
- 这两块在首次进入时并不渲染：
  - 文件编辑器只在文件标签激活时渲染（`CodingWorkspace.vue:202`）
  - 终端只在用户点击终端按钮后渲染（`CodingWorkspace.vue:196`）
- 生产构建实测体积：
  - `editor.api-*.js` ≈ 2.67 MB（Monaco 核心）
  - `CodingWorkspace-*.js` ≈ 1.74 MB
  - `CodingWorkspace-*.css` ≈ 88 KB，`editor-*.css` ≈ 97 KB
- dev 模式额外成本：`vite.config.ts` 中 `optimizeDeps.exclude: ['monaco-editor']`，Monaco 按源码模块逐个加载/转换；`src/main.ts:29-33` 启动时还预导入 5 个 Monaco worker。
- 进入路由后的渲染/副作用高峰：`SidebarPanel` 全量渲染会话列表；`NewSessionDialog` 常驻挂载并在 `onMounted` 调用 `engineStore.load()`（`NewSessionDialog.vue:40-42`），且该弹窗在侧栏/顶栏/工作区存在多个实例。
- 首页功能入口由 `HomeView.vue` 的 `features` 数组驱动，注释明确「后续新功能只需在此追加」，因此写作等功能会沿用同一入口模式。

## Requirements

### R1 入口重依赖异步化

`/coding` 首屏阶段不得下载/执行 Monaco 与 xterm。文件编辑器与终端改为按需加载（异步组件），首屏只加载工作区外壳与会话相关代码；空闲预取发生在首帧之后。

### R2 预取与 loading 体验

异步化后，首次打开文件/终端不能在无反馈的情况下空白：
- 进入 `/coding` 首帧之后、浏览器空闲时预取编辑器 chunk；终端 chunk 只在悬停/点击终端按钮时预取，避免无谓加载。
- 文件树、终端按钮等触发点支持 hover/临近交互时提前触发加载。
- 异步组件加载期间展示明确的 loading/骨架，不使用会阻塞整块工作区的全局 `Suspense`。

### R3 开发模式成本

降低 dev 下首次进入 `/coding` 的模块转换与请求数，且不破坏 Monaco worker 配置与生产构建。

### R4 后续功能入口约定

为「写作」等后续功能确立约定：新功能使用独立懒加载路由；编辑器、终端、画布等重依赖只能出现在各自功能的异步子模块中，不得进入首页或共享布局的静态导入链。约定需可被后续任务直接复用（spec 或任务内文档）。

### R5 行为不回归

- 文件打开、编辑、Ctrl+S 保存、dirty 状态、冲突提示保持现有行为。
- 终端占位行为、开合逻辑保持现有行为。
- 会话切换、消息加载、侧栏/顶栏布局与交互不受影响。

## Acceptance Criteria

- [ ] 生产构建下首次点击「编码」到工作区可见，首屏加载完成前不请求 Monaco（`editor.api`）与 xterm 相关 chunk。
- [x] `/coding` 入口 JS 体积显著下降（以拆分后 chunk 清单与体积对比为证，记录改前/改后数值）。
- [ ] 首次打开文件时先出现 loading，随后编辑器可用；在已预取的情况下打开文件无明显白屏。
- [ ] 首次打开终端时先出现 loading/占位，随后终端可用；不触发整页卡死。
- [ ] dev 模式首次进入 `/coding` 的等待时间相对改前有可测改善（记录观察方式与数值）。
- [ ] 现有文件编辑/保存/冲突、终端占位、会话切换流程手动验证通过，无控制台报错。
- [x] 产出「新功能入口懒加载约定」，能直接指导后续写作功能落地。

> 构建级 AC 已完成（`pnpm build` 通过、入口无 Monaco/xterm 静态依赖、体积已记录）；交互式验证项待 `pnpm dev:tauri`。

## Scope Decision

范围取 **B 档：入口拆分 + 开发模式 + 长期约定**（已与用户确认）：

- 包含：Monaco/xterm 异步化、空闲/悬停预取、loading 反馈。
- 包含：开发模式下把 Monaco worker 初始化移出启动路径，并记录改造前后 dev 首进耗时。
- 包含：产出「新功能入口懒加载」约定文档，指导后续写作等功能。
- 不包含：侧栏会话列表虚拟滚动、`NewSessionDialog` 按需挂载等进入后的渲染高峰优化。

## Out of Scope

- 不新增写作功能本身。
- 不修改 ACP 会话协议、消息存储、任务看板、自动化等业务逻辑。
- 不改 `optimizeDeps.exclude: ['monaco-editor']`（需单独验证预打包与 worker 的兼容性；本任务只做异步拆分并把 worker 初始化移出启动路径）。
- 不引入 `manualChunks` 全量构建重构、侧栏虚拟滚动、`NewSessionDialog` 按需挂载。
