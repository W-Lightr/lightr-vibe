# 接入 Claude Code 与 Codex ACP 适配器引擎

父任务：`10-08-real-agent-workspace`。前置：`10-13-acp-multi-engine`（已归档，引擎注册表
地基：`AgentType` 枚举 + `AcpAgentMeta` + `get_agent_meta` + `AcpConn.engine` + 前端 engine store）。

## Goal / 用户价值

在既有引擎注册表上接入**两个真实引擎**：Claude Code 与 Codex。它们**原生 CLI 不说 ACP**，
需通过官方 ACP 适配器包桥接（`@agentclientprotocol/claude-agent-acp` /
`@agentclientprotocol/codex-acp`）。**照 codeg：用户自行安装厂商 CLI 与适配器，本体不打包、
不自动下载**，我们只解析并启动。使用户能在新建会话时选择 Claude / Codex，像 opencode 一样
获得流式回复、工具面板、权限、模型切换、历史恢复等既有能力。

## Background / Confirmed facts（证据见 `research/codeg-claude-codex.md`）

- **适配器事实**：`claude`/`codex` 原生 CLI 不支持 ACP；codeg 走官方适配器包（npx 分发），
  适配器命令名 `claude-agent-acp` / `codex-acp`，与厂商 CLI 名无关；共享厂商配置目录
  `~/.claude` / `~/.codex`，装适配器无需二次登录。**适配器依赖树含厂商 SDK/CLI**
  （`claude-agent-acp` 带 `@anthropic-ai/claude-agent-sdk`，装完 ~287MB；`codex-acp` 带
  `@openai/codex`）→ **不宜打包自带**，codeg 也不打包。
- **分发形态差异**：opencode 是 `opencode acp`（Binary，PATH 直启）；claude/codex 是用户
  `npm i -g` 装的适配器命令 → 启动时先解析「适配器命令真身」（PATH → npm 全局 bin），再 spawn。
- **本项目地基已就绪**：`agent_type.rs`（枚举+wire）、`engine.rs`（`AcpAgentMeta`+`LaunchSpec`
  +`resolve_spawn`）、`acp.rs`（`build_acp_command(meta)` + `AcpConn.engine`）、
  `agent.rs`（`agent_detect(engineId)`）、前端（`engineService`/`stores/engine.ts`/
  `NewSessionDialog`/`settings.json` 默认引擎）。**本期核心 = 补 2 个枚举变体 + 扩展
  `LaunchSpec` 支持「外部适配器命令」形态 + 全链路实测回归**。

## Resolved Decisions（用户已确认）

- **D1 照 codeg：用户自装 CLI + 适配器**（用户选定）：**本体不打包、不自带、不自动下载**。
  我们只解析适配器命令（PATH → npm 全局 bin）；解析不到 → 报错 + `npm i -g <package>` 指引。
- **D2 复用引擎注册表，不重写协议层**：两枚新引擎 = 新 `AgentType` 变体 + 一条
  `get_agent_meta` match 臂 + `LaunchSpec` 加外部适配器分发字段。协议路由（`acp.rs` 的
  reader_loop/handle_incoming/…）**不引用引擎名**（延续 AC8）。
- **D3 砍掉 codeg 引擎专属能力**（本期 Out of Scope）：steering/goal/AIR sessionFailure/
  elicitation/plan/codex env 策略/权限 changes 渲染等一律不做。仅保证**通用 ACP 路径**可用。
- **D4 wire 名不可变**：Claude → `"claude_code"`，Codex → `"codex"`（**照 codeg**
  `models/agent.rs:95,96` 的既有持久化契约，便于将来与 codeg 数据互通语义一致）；加单测锁死。
- **D5 检测降级可用性优先**：适配器是否支持 `--version`/`--help` 未定 → 检测以
  「能否 resolve 到命令」为 installed 判据；版本/ACP 探测 best-effort，不阻塞可用性。

## Requirements

### R1 Rust：两枚新引擎枚举 + wire
- `AgentType` 增加 `ClaudeCode`、`Codex` 变体；`as_wire` → `"claude_code"` / `"codex"`；
  `from_wire` 反向；`BUILTIN_AGENT_TYPES` 补全。
- 加/更新单测：wire 名不可变、往返、未知拒绝、`BUILTIN` 覆盖所有变体。

### R2 Rust：LaunchSpec 支持外部适配器分发
- 扩展 `LaunchSpec`（新增 `LaunchKind`）以表达「命令真身来自 PATH 或 npm 全局 bin 的用户
  自装适配器」这一形态；opencode 保持现状形态不变，回归零破坏。
- 新增适配器命令解析：PATH 扫描（复用 `resolve_bin` 候选机制）→ npm 全局 bin 扫描
  （`npm prefix -g` + `<prefix>/bin` 下 `cmd.cmd`/`cmd.exe`/`cmd`）。解析不到 → 明确错误 +
  安装指引；**不触发任何下载**。
- Windows `.cmd` shim 复用既有 `extract_exe_from_shim` / `resolve_spawn` 回退链。
- claude/codex 的 `AcpAgentMeta`：name/description/bin_candidates（`claude-agent-acp*` /
  `codex-acp*`）/env_strip（空或按需）/detect_args/acp_probe_args/config_path
  （`~/.claude` / `~/.codex` 下的具体文件或目录）/context_window（先 `None`）。

### R3 Rust：命令面/检测
- `acp_connect` / `acp_probe_options` / `agent_model_context_window` / `agent_detect` /
  `agent_read/write_config` **已带 engineId 参数**（上一任务完成）→ 只需保证新引擎走通。
- `acp_list_agents` 自动列出 3 个引擎（`BUILTIN_AGENT_TYPES` 驱动），无需改逻辑。
- 检测 new：适配器命令 resolve + best-effort 版本/help 探测（D5）。

### R4 前端：引擎列表与选择
- `engineService.mockList` / `stores/engine.ts` 静态兜底补 Claude/Codex 两项。
- `NewSessionDialog` 引擎下拉自然列出 3 个引擎（列表来自 `acp_list_agents`）。
- `useConversation` 的模型键 / auth 文案 / agentName 兜底按会话 `engineId` 取（上一任务
  已参数化；补 claude/codex 的显示名/占位映射）。
- `AgentManageView` 注册表驱动（上一任务已做；补新引擎的通用详情占位）。

### R5 持久化
- `Session` / `SessionIndexEntry` 的 `engineId` 已支持（上一任务）；新引擎取值
  `"claude_code"` / `"codex"`；旧数据默认 `"open_code"` 不变。

### R6 文档/文案
- 明确「引擎 vs 适配器」的用户可见说明：设置页/新建弹窗提示用户这两个引擎经官方适配器启动。

## Acceptance Criteria

- [ ] AC1 `cargo check` + `cargo test --lib` + `vue-tsc --noEmit` 全绿。
- [ ] AC2 `AgentType::{ClaudeCode,Codex}` 的 `as_wire`/`from_wire` 往返；未知 wire 拒绝；
      `BUILTIN_AGENT_TYPES` 覆盖全部变体；wire 名单测锁死（`claude_code` / `codex`）。
- [ ] AC3 `acp_list_agents` 返回 3 项（open_code / claude_code / codex），`available` 按实机探测。
- [ ] AC4 新建会话弹窗可选 Claude / Codex；创建的会话 `engineId` 正确落库。
- [ ] AC5 Tauri 实机：Claude 会话与 Codex 会话各能建连 + 发送 prompt + 收到流式回复 +
      工具面板/停止/权限/模型下拉（**通用能力不回归**）。
- [ ] AC6 opencode 会话行为与改造前完全一致（回归零破坏）。
- [ ] AC7 浏览器 mock：3 引擎列表 + 全 ACP 流程不崩溃。
- [ ] AC8 协议路由代码（`acp.rs` reader_loop/handle_incoming/handle_client_request）
      **不含引擎名分支**；引擎差异仅存在于注册表声明与 spawn 解析。
- [ ] AC9 未安装适配器时：检测报 unavailable + 明确的错误文案（含 `npm i -g <package>` 安装
      指引，非崩溃），新建弹窗该项标记不可用并 fallback 到可用引擎。
- [ ] AC10 前端的引擎兜底列表、mock、settings 默认引擎读写对新引擎生效。
- [ ] AC11 **不自带/不下载**：实现中无打包适配器、无 npm install/npx 下载调用；仅解析已装命令。

## Out of Scope（明确排除 codeg 重型机制）

- 引擎专属能力：steering / goal / AIR sessionFailure / elicitation / plan 模式 /
  权限 `_meta.changes` 渲染 / contextCompaction 卡 / fork / 子代理。
- codex `apply_codex_env_policy`（MCP 过滤 / INITIAL_AGENT_MODE）——本期无 MCP 注入，不做。
- **适配器/厂商 CLI 的打包、自带、自动下载、安装 UI、版本管理**（照 codeg：一律用户自装）。
- 引擎的下载/安装/多平台二进制分发（`Binary` 多平台 URL、`Uvx`）。
- 用户自定义引擎（`Custom` + `custom_registry` + DB）。
- 密钥登录流程改造。
- claude/codex 的 context-window 精确解析（先返回 None，非阻塞）。
- 每引擎独立 DB agent_setting 表。

## Open Questions（待实现阶段实测，见 research §7）

- 适配器是否支持 `--version` / `--help`（决定 detect/probe args）。
- 适配器（含 SDK）首连握手耗时（首连超时矩阵是否上调）。
- `session/new` 的 `configOptions` 形状是否与 opencode 一致。
- 适配器凭据来源（共享 `~/.claude`/`~/.codex` 是否够）。
