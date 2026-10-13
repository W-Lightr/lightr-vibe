# 多引擎 ACP 抽象与新建会话选引擎

父任务：`10-08-real-agent-workspace`。前置：`10-08-opencode-acp-chat`（已归档，
per-session 进程模型）。本任务在既有 ACP 链路上做**引擎插件化改造**，不重写协议层。

## Goal / 用户价值

当前 ACP 链路把「引擎」硬编码为 opencode：进程命令、env 过滤、检测、配置路径、
模型 context-window、前端设置页与模型缓存全部只认 opencode。本任务引入**引擎
注册表（engine registry）**抽象，使：

1. 新建会话时**可选引擎**，会话绑定一个引擎，后续该会话的进程/探测/配置都按所选引擎执行；
2. 新增一个 ACP 兼容引擎 = 在注册表加一条声明，无需改动 acp.rs 协议路由/前端消费面；
3. 首批引擎仍只有 opencode（回归零破坏），但架构为后续 Claude Code / Gemini /
   Codex 等 ACP 适配器预留落点。

## Background / Confirmed facts（证据：代码勘察）

**引擎硬编码点（file:line）**

- Rust 进程命令：`acp.rs:130-206`（`resolve_bin` 候选 `opencode.exe/.cmd/.bat`、
  `build_acp_command` 写死 `arg("acp")` 与 `cmd /C opencode acp`、`env_remove("OPENCODE_SERVER_*")`）。
- Rust 检测/配置：`agent.rs:75-77`（`~/.config/opencode/opencode.jsonc`）、
  `agent.rs:100-116`、`agent.rs:192-215`（`Command::new("opencode")`）、
  `agent.rs:323-375`（`detect_opencode_inner`：`--version` + `acp --help`）。
- Rust 模型 context-window：`acp.rs:939-1115`（`$XDG_CONFIG_HOME/opencode`、
  `$OPENCODE_MODELS_PATH`、`$HOME/.cache/opencode/models.json`）。
- 命令面**无引擎参数**：`lib.rs:19-43`、`acp.rs:1121-1370`、`agent.rs:305-447`。
- 结构体**无引擎字段**：`acp.rs` structs、`agent.rs` structs、
  `types/session.ts:51-78`（`Session`）、`types/acp.ts:162-173`（`SessionIndexEntry`）。
- 前端单引擎假设：`useConversation.ts:141`（`MODEL_AGENT_KEY='opencode'`）、
  `useConversation.ts:257`（`opencode auth login`）、`stores/agent.ts`（单 detect 结果）、
  `stores/workspace.ts:8-18`（静态 `MODELS`/`EXECUTION_MODES`）、
  `MessageComposer.vue:290-291`（`agentName` 兜底 `'OpenCode'`）、
  `AgentManageView.vue:23-38`（写死 `id:'opencode'`，`selectedAgentId` 非响应式）。
- 新建会话流程**只绑 model+cwd**：`stores/session.ts:220-237`、
  `CodingWorkspace.vue:26-35`、`SidebarPanel.vue:97-113,152-168`、`TopBar.vue:33-42`。
  引擎未选择、未存储；连接时隐式 spawn 单一 opencode 进程
  （`useConversation.ts:415-499` → `acpService.connect`）。
- 持久化：`~/.lightr-vibe/lightr-vibe/sessions-index.json`，记录字段
  `id/title/kind/projectId?/cwd?/acpSessionId?/model/mode?/createdAt/updatedAt`
  （`stores/session.ts:55-68` `toIndexEntry`；`parseIndex` 容忍未知字段）。**无 engine 字段。**
- 既有 spec 契约：`.trellis/spec/frontend/tauri-bridge.md` §ACP（per-session 进程、
  connId 路由、spawn/probe/context-window、8 项不变量）。

**协议事实**：ACP 为 newline-delimited JSON-RPC；不同引擎的差异集中在**进程如何启动**、
**env 如何清洗**、**模型从哪来**（`session/new` 的 `configOptions` 是协议标准，但
context-window 等元数据各引擎存放位置不同）。

**参考实现（codeg，只读）**：`H:\AI\Light-vibe\codeg`
- `src-tauri/src/models/agent.rs:23` —— `AgentType` 编译期枚举 + `as_wire`/`from_wire`
  持久化契约（15 内置 + `Custom`），`builtin_wire_names_are_unchanged` 单测锁 wire 名。
- `src-tauri/src/acp/registry.rs:130` —— `AcpAgentMeta` + `get_agent_meta` 静态查表；
  `AgentDistribution{Npx|Binary|Uvx}` 三种分发（含多平台下载，本期不做）。
- `src-tauri/src/acp/connection.rs:1861` —— `build_agent` 按 `match meta.distribution`
  构造命令，并含大量 `if agent_type == …` 专属分支（本期须避免）。
- `src-tauri/src/acp/manager.rs` —— `ConnectionManager`，连接带 `agent_type`。
- `src-tauri/src/acp/types.rs:1504` —— `AcpAgentInfo`（列表下发形状）。
- `src/hooks/use-acp-agents.ts` —— 前端单一共享 agent store（zustand，ref-counted）。
- `src/components/chat/agent-selector.tsx:93` —— 选择器 + `defaultAgentType` 不可用时
  `onFallback` 语义（fallback ≠ 用户确认）。
- `src-tauri/src/db/entities/agent_setting.rs` —— 每 agent 一行配置（enabled/sort/
  installed_version/env）。

## Resolved Decisions（用户已确认）

- **D1 完整引擎插件化（照 codeg 架构）**：spawn / 检测 / 模型探测 / context-window /
  env 过滤 **全部**引擎化；采用 codeg 的三件事——① **编译期枚举 `AgentType` + `as_wire`/
  `from_wire` 持久化契约**；② **静态元数据注册表**（`AcpAgentMeta` + `get_agent_meta` 查表）；
  ③ **前端单一共享 agent store**。**但砍掉** codeg 的重型部分（下载/安装/多平台分发/
  DB/自定义 agent）——见 Out of Scope。
- **D2 新建会话时选引擎**：引擎在会话创建时确定，会话内不切换（对齐 ACP 语义，
  切换=杀进程重建，成本高）。
- **D3 引擎身份用枚举 + wire 契约**（照 codeg `AgentType::as_wire`）：会话持久化存
  wire 字符串（opencode → `"open_code"`），Rust 侧枚举化；**wire 名不可变**并加单测
  锁死（改名会孤立已存数据）。未知 wire → 报错。
- **D4 会话数据模型加 `engineId`**：`Session` 与 `SessionIndexEntry` 增加 `engineId`
  （存 wire 值）；旧持久化数据缺该字段时默认回退 opencode（向后兼容）。
- **D5 新建会话用弹窗选引擎**；无记忆偏好时默认选**设置页的「默认引擎」**设置项
  （非"上次使用"）。用户单次选择的引擎不回写默认。照 codeg `AgentSelector`：默认引擎
  不可用时 fallback 到首个可用引擎；**fallback 不等于用户确认**，不回写默认。
- **D6 引擎专属逻辑优先数据驱动**：能声明进 `AcpAgentMeta` 的（spawn/env/检测/路径）
  一律入表；**警惕** codeg `build_agent` 里海量 `if agent_type == …` 分支，本期 opencode
  若产生分支必须是极少且必要。

## Requirements

### R1 引擎注册表（Rust，core，照 codeg `models/agent.rs` + `acp/registry.rs`）
- 新增 `AgentType` **编译期枚举**（本期仅 `OpenCode` 变体），实现 `as_wire()` /
  `from_wire()` / `Display` / `Serialize` / `Deserialize`；wire 名 `"open_code"`，
  未知名报错；加「wire 名不可变」单测。
- 新增静态元数据结构 `AcpAgentMeta { agent_type, name, description, launch }` 与
  `get_agent_meta(AgentType) -> AcpAgentMeta` 查表函数；本期 opencode 一条。
  `launch` 声明：bin 候选、ACP 子命令、env 清洗列表、检测命令、配置路径规则。
- `resolve_bin` / `build_acp_command` / `resolve_spawn` 改为**按 `AcpAgentMeta`** 解析。
- `env_remove` 列表来自声明（opencode：`OPENCODE_SERVER_USERNAME/PASSWORD`）。
- 无匹配变体时无法构造（枚举保证）；wire 解析失败返回明确错误。
- **收敛** `acp.rs` / `agent.rs` 两处重复的 `resolve_bin` 到注册表单一实现。

### R2 命令面加引擎维
- `acp_connect` / `acp_probe_options` / `agent_model_context_window` 等需要引擎上下文的
  命令增加引擎参数（wire engineId）；**会话级命令**（session_new/load/prompt/…）继续以
  connId 定位，引擎从该连接记录取（`AcpConn` 加 `engine: AgentType`，连接时绑定），
  命令面不必重复传。
- `agent_detect_opencode` → 泛化为 `agent_detect(engineId)`；配置读/写命令同理引擎化。
- 新增 `acp_list_agents`（照 codeg `acp_list_agents_core`）→ 返回 `AcpAgentInfo[]`
  （本期仅 opencode 一项）：`{ agent_type, name, description, enabled, sort_order,
  installed_version?, available, ... }`。
- lib.rs 注册同步更新。

### R3 检测与配置引擎化
- 检测（installed / version / acpOk / binPath / configPath）按 `AcpAgentMeta` 执行；
  opencode 检测逻辑保持现状语义（`--version` + `acp --help`）。
- 配置读取/写入路径来自声明（opencode：`~/.config/opencode/opencode.jsonc`）。

### R4 模型 context-window 引擎化
- `agent_model_context_window` 的配置/缓存查找策略改为引擎声明的一部分；
  opencode 策略保持现状（用户 config → models.dev 缓存）。

### R5 前端：新建会话弹窗选引擎
- 新建会话入口（`SidebarPanel.vue` / `CodingWorkspace.vue` / `TopBar.vue`）触发一个
  **选择弹窗**：选引擎（本期列表仅 opencode），确认后创建会话。
- 弹窗默认预选 = 设置页的「默认引擎」；不可用时 fallback 到首个 available（照
  `AgentSelector` 的 `onFallback` 语义，不把 fallback 当用户选择）。
- 引擎列表来自 `acp_list_agents`（经命令下发），前端不写死列表。
- 创建时把 `engineId`（wire）写入 `Session`。
- **默认引擎设置项**：设置页 Agent 管理增加「默认引擎」选择，持久化到全局设置
  （`~/.lightr-vibe/lightr-vibe/` 命名空间内，新增 `settings.json`）。

### R6 前端：消费面按引擎 + 单一共享 store
- 新增 `stores/engine.ts`（照 codeg `use-acp-agents`）：**单一共享 store**，挂载时
  `acp_list_agents` 拉取、排序（sortOrder + name）、缓存；失败保留上一次/回退静态 opencode。
- `useConversation` 的 `MODEL_AGENT_KEY='opencode'` 常量改为按会话 `engineId` 取键。
- 认证/门禁提示文案按引擎（不写死 `opencode auth login`）。
- `MessageComposer` 的 agentName 兜底与占位文案按会话引擎。

### R7 设置页：Agent 管理引擎化
- `AgentManageView` 的写死 `agents[]`/`selectedAgentId` 改为从 `stores/engine.ts` 渲染、
  可响应选择；opencode 详情页保持现有能力，其余引擎（暂无）走通用详情占位。
- 增加「默认引擎」选择写 `settings.json`。

### R8 持久化向后兼容
- `SessionIndexEntry` / `Session` 增加 `engineId`（wire 字符串）；`toIndexEntry`/
  `sessionFromEntry` 读写；旧记录缺字段 → 默认 opencode。`forkSession` 复制 `engineId`。
- **不破坏** 命名空间约束（仅 `~/.lightr-vibe/lightr-vibe/`）。

## Acceptance Criteria

- [ ] AC1 `cargo check` + `cargo test --lib` + `vue-tsc --noEmit` 全绿。
- [ ] AC2 新建会话时可选择引擎（本期列表仅 opencode）；创建的会话 `engineId=opencode`。
- [ ] AC3 Tauri 实机：新建 opencode 会话发送消息，行为与改造前**完全一致**
      （流式回复 / 工具面板 / 停止 / 权限 / 模型切换 / 重启恢复）。
- [ ] AC4 引擎选择链路可验证：**不 spawn 假引擎**（`AgentType` 是编译期枚举，加假变体会
      引入无法启动的进程）。改为：① 纯前端 mock 校验「弹窗选择 → createSession(engineId)
      → 持久化 engineId」链路；② Rust 单测校验 `get_agent_meta`/`as_wire` 契约。
      （原「注册表新增 mock 引擎」表述作废——见 design §3 诚实标注。）
- [ ] AC5 旧 `sessions-index.json`（无 engineId）加载后默认 `opencode`，会话可继续。
- [ ] AC6 设置页 Agent 管理：列表由注册表驱动，选择响应式；opencode 详情功能不回归。
- [ ] AC7 浏览器 mock：新建会话引擎选择 + 全 ACP 流程不崩溃（mock 数据齐备）。
- [ ] AC8 命令面无「引擎名」硬编码残留：`rg -i "opencode"` 在 `acp.rs`/`lib.rs`
      仅出现在注册表声明与必要的引擎专属实现处，协议路由代码不含引擎名分支。
- [ ] AC9 Rust 单测：`AgentType::OpenCode.as_wire()=="open_code"` 且 `from_wire` 往返；
      未知 wire 解析报错；`get_agent_meta` 对每个 `BUILTIN` 变体都返回一致
      `agent_type`（照 codeg `builtin_wire_names_are_unchanged` 测试风格）。
- [ ] AC10 种子会话（`stores/session.ts:17 seedFolderSession`）也带 `engineId` 落库，
      不产生 `engineId: undefined` 的脏记录。

## Out of Scope（明确排除 codeg 的重型机制）

- 除 opencode 外的真实引擎适配器实现（Claude Code / Gemini / Codex 等）。
- **引擎的下载 / 安装 / 版本管理 / 多平台分发**（codeg 的 `AgentDistribution::Binary/Npx/Uvx`、
  二进制缓存、下载校验）——本期只 spawn「系统 PATH 上已安装」的引擎。
- **用户自定义引擎**（codeg `Custom(&'static str)` + `custom_registry` + DB）。
- **引擎能力协商差异**（如无 configOptions 的引擎、各 agent 专属 launch 分支/工具策略）。
- 会话内切换引擎；跨引擎会话迁移/fork 到另一引擎。
- **每引擎独立 DB 设置表**（codeg `agent_setting`）——本期用一个 `settings.json` 存默认引擎。

## Open Questions

- （无阻塞项；设计阶段技术选型见 design.md）
