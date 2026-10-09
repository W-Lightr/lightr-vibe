# 父任务：真实 Agent 会话与文件夹管理

**父任务职责**：持有总需求、子任务地图、跨子验收与集成评审。父任务本身不直接实现
（实现落在两个子任务）。

## Goal / 用户价值

编码工作区从「全 mock 演示」升级为「真实能力」：
1. 侧栏可打开真实文件夹作为项目，文件树/编辑器读写真实磁盘；
2. 对话区经 OpenCode ACP 驱动真实 Agent 会话（流式回复、工具调用、停止）。

## 子任务地图

| 子任务 | 交付 | 依赖 |
|---|---|---|
| `10-08-real-folder-projects` | 打开文件夹（原生对话框）+ 真实目录树/文件读写落盘 + 项目持久化 | 无 |
| `10-08-opencode-acp-chat` | ACP stdio JSON-RPC 会话链路（发送/流式/工具/停止/权限）+ 会话持久化 | 弱依赖子1：会话需要 cwd（真实项目路径）；无子1 时可用 tmpsession 目录先行 |

顺序建议：先子1（小、独立、立即修复用户报的按钮无响应 bug），后子2。
两者文件面重叠小（子1: project store/FilesTree/FileEditorView/Rust fs；
子2: conversationService/useConversation/session store/Rust acp 进程管理）。

## Confirmed facts（证据）

- **Bug 确认**：`SidebarPanel.vue:196-200`「打开文件夹」按钮**无 @click 绑定**
  （纯装饰），故点击无反应；项目数据全为 `stores/project.ts` 的 mockProjects。
- **编辑不落盘**：`FileEditorView.vue:31` onChange → `projectStore.setFileContent`
  仅写内存 mock；无保存按钮/快捷键。
- **会话全 mock**：`services/conversationService.ts` 的 send() 用定时器伪造
  thinking/tool/delta 事件；`useConversation.ts` 消费事件流写 session store。
  事件映射点 `applyEvent(sessionId, turnId, MockEvent)` 是替换 seam。
- **ACP 入口存在**：opencode 1.18.35 有 `acp` 子命令（stdio ACP server）；
  协议调研由 trellis-research 进行中（research/ 将产出契约）。
- **既有桥接规范**：`.trellis/spec/frontend/tauri-bridge.md`（命令风格、camelCase、
  isTauri 双通道、.cmd 回退链、conflict 语义）——两个子任务都必须遵循。
- **窗口/拖拽契约**：ui-style.md / component-guidelines.md（data-no-drag 等）。
- **Tauri 依赖现状**：无 dialog 插件、无 fs 插件；capabilities 仅 sidecar shell。
- 设计图侧栏含「最近」分组（暗示项目/会话需跨启动持久化）。

## 跨子任务验收（集成评审时核对）

1. 打开一个真实文件夹 → 侧栏出现该项目 → 树可展开真实目录 → 双击文件在
   Monaco 打开真实内容 → 修改保存后磁盘文件确实变化。
2. 在该项目下新建会话发送消息 → 对话区出现真实 Agent 流式回复与工具调用面板
   内容（来自 ACP session/update），停止按钮可中断。
3. 重启应用：项目列表与会话列表恢复；会话可继续（ACP session/load 或新会话，
   按子2 设计）。
4. 浏览器 dev：两子功能均回退 mock 且有提示条，不崩溃。
5. 全程不破坏设置页/Agent 管理已有功能（回归）。

## Out of Scope

- 多 Agent 并发会话编排、会话分叉、跨项目搜索。
- opencode TUI/serve 模式集成（仅 acp stdio）。
- 文件编辑的 diff 视图增强、git 集成（设计图 DiffViewer 保持现状）。

## Resolved Decisions（用户已确认 2026-10-08）

- Q1 Agent 未安装/检测失败：发送消息给引导提示（跳设置页），**不产生假回复**；
  检测 PASS 才走真实 ACP。
- Q2 会话持久化：Lightr-vibe 自存索引 + 消息落盘（重启可恢复、可继续）。
- Q3 文件夹项目跨启动持久化 + 侧栏「最近」分组。

## 数据命名空间安全约束

`~/.lightr-vibe/` 下既有 `projects.json` / `sessions.json` / `messages/` 为**其他
应用**的数据（schema 为 agent-<ts> 键映射，非本项目写入）。本项目一律使用
`~/.lightr-vibe/lightr-vibe/` 子目录命名空间，**禁止读写上述既有文件**。

## ACP 调研结论（子2 规划基线）

见 `../10-08-opencode-acp-chat/research/acp-protocol.md` 与 `opencode-acp-probe.md`：
newline-delimited JSON-RPC stdio；session/new 首调 7.6-8.9s（超时 ≥30s）；模型经
configOptions 暴露（无 list_models）；cancel 44ms 生效；stdin 关闭干净退出；
Windows 直 spawn 真身 exe + CREATE_NO_WINDOW，.cmd shim 需解析回退。

## 集成评审（2026-10-09 主会话核对）

| 跨子 AC | 状态 | 证据 |
|---|---|---|
| 1 打开真实文件夹 → 树 → 打开文件 → 保存落盘 | ⏳ 待人工（AC2-6） | 子1 六 cargo 单测覆盖越界/冲突/原子写；UI 路径待 GUI 点验 |
| 2 项目下新建会话 → 真实流式 + 工具面板 + 停止 | ⏳ 待人工 | 子2 probe 真机 end_turn 往返 + usage 实测；UI 待点验 |
| 3 重启恢复项目与会话、可继续 | ⏳ 待人工 | 两侧持久化实现 + 命名空间静态验证通过 |
| 4 浏览器 dev 双功能回退 mock 且有提示条 | ✅ | vite dev transform 200（/coding、/settings、全部新模块） |
| 5 不破坏设置页/Agent 管理（回归） | ✅ | git diff 证实设置页/agent store/fs.rs(子2) 零改动；pnpm build 全绿 |

已修复的跨子集成问题：
- `tmpSessionDirSync()` 原指向 `~/.lightr-vibe/tmpsession`（根，属其他应用面）→
  子2 核查修订为命名空间内 `~/.lightr-vibe/lightr-vibe/tmpsession`。
- 子1 遗留的「移除项目时会话解绑」组件层直改 → 子2 收敛为
  `sessionStore.orphanSessionsOf` action。
- 子2 会话创建需项目 rootPath 作 cwd → 三处 createSession 调用点已接。
