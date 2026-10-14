# Implement: 接入 Claude Code 与 Codex ACP 适配器引擎

需求见 `prd.md`，设计见 `design.md`，事实见 `research/codeg-claude-codex.md`。
执行顺序：Rust 先（编译期变体→注册表→spawn→检测），前端后；每步可独立验证。

## 验证命令（本项目基准）

```powershell
# Rust
cargo check            # workdir: src-tauri
cargo test --lib       # workdir: src-tauri
# 前端
pnpm vue-tsc --noEmit
pnpm build
```

> 单一来源：`cargo test --lib` 覆盖 `agent_type.rs` / `engine.rs` 单测。
> 若配置了 `pnpm lint` 一并跑。

---

## Step 0 · 前置实测（阻塞设计定稿，先做）

**前提**：用户已自装厂商 CLI 与适配器（`npm i -g @agentclientprotocol/claude-agent-acp`
与 `@agentclientprotocol/codex-acp`）。若未装，先在终端装好（这是"用户自装"路线的动作，
**不是 app 行为**）。

在写 spawn 代码前，先在终端确认适配器行为（避免按猜测写 detect args）：
1. `claude-agent-acp --version` 与 `--help` 是否可用？（PATH 或 `npm prefix -g` 的 bin 下）
2. `codex-acp --version` 与 `--help`？
3. 起进程能否用一次 `initialize`+`session/new` 握手取到 configOptions？
   （可临时手写 JSON-RPC 或等 Step 4 后跑 `acp_probe_options`）。
4. 首连握手耗时（计时；决定超时矩阵是否上调）。
5. Windows：`npm prefix -g` 输出与 bin 目录形态（`.cmd`？）；适配器命令真身路径。
6. 厂商 CLI 可用性：`claude --version` / `codex --version`（适配器需能调到厂商 CLI）。

**产出**：把结果回填 `research/codeg-claude-codex.md` §7；据此**定稿** design §3.5 的
`detect_args` / `acp_probe_args` / `env_strip` / `config_path`。

---

## Step 1 · Rust：AgentType 扩展（`agent_type.rs`）

- [ ] 加 `ClaudeCode` / `Codex` 变体；`as_wire` → `"claude_code"` / `"codex"`；
      `from_wire` 反向；`BUILTIN_AGENT_TYPES` 补全。
- [ ] 更新/加单测：`builtin_wire_names_are_unchanged` 覆盖三对；`wire_roundtrip_*`；
      `unknown_wire_is_rejected`（补 `"claude"`、`"claude_code_x"` 等仍拒绝）。

**验证**：`cargo test --lib`（agent_type 模块）。
**回滚点**：仅本文件，可单文件 revert。

## Step 2 · Rust：LaunchSpec 扩展（`engine.rs`，纯数据+纯函数）

- [ ] 新增 `LaunchKind { DirectCommand, ExternalAdapter }`。
- [ ] `LaunchSpec` 加 `kind: LaunchKind` + `install_hint: Option<&'static str>`。
- [ ] `OPENCODE_META` 填 `kind: DirectCommand` / `install_hint: None`（回归不变）。
- [ ] 加 `CLAUDE_CODE_META` / `CODEX_META`（bin_candidates / acp_subcommand:"" / env_strip /
      detect_args / acp_probe_args / config_path:None / context_window:None /
      kind:ExternalAdapter / install_hint）。字段值以 Step 0 实测为准。
- [ ] `get_agent_meta` 补两条 match 臂。
- [ ] 单测：`meta_matches_builtin_variants` 自动覆盖；补 claude/codex 关键字段断言。

**验证**：`cargo test --lib`（engine 模块）。
**回滚点**：本文件。

## Step 3 · Rust：npm 全局 bin 解析 + resolve_spawn 扩展（`engine.rs`）

- [ ] 纯函数：`npm_bin_dir_from_prefix(prefix, windows) -> PathBuf`（Windows=`prefix`，
      Unix=`prefix/bin`）——**可单测**（不真跑 npm）。
- [ ] `resolve_npm_global_bin_dir()`：跑 `npm prefix -g`（带超时，失败 None）。
- [ ] `candidate_in_dir(dir, bin_candidates) -> Option<String>`：按候选找真身。
- [ ] `resolve_bin_in_npm_dir(meta)`：`resolve_npm_global_bin_dir` + `candidate_in_dir`。
- [ ] `SpawnSpec` 加 `NotInstalled { hint: String }`。
- [ ] `resolve_spawn(meta)` 扩展：
      - `DirectCommand`：现状链不变（+ 非空 subcommand 追加）。
      - `ExternalAdapter`：PATH → npm 目录 → 全未命中 `NotInstalled`。
- [ ] 单测：`npm_bin_dir_from_prefix` 两平台；`resolve_spawn` 对 ExternalAdapter 用注入候选
      （测试专用 helper，避免真跑 npm）验证三态。

**验证**：`cargo test --lib`。
**回滚点**：`engine.rs`；`SpawnSpec` 新变体只在本步骤后使用。

## Step 4 · Rust：build_acp_command 按 SpawnSpec 构造（`acp.rs`）

- [ ] `build_acp_command` 处理三态：
      - `Exe(p)`：`Command::new(p)`；DirectCommand 时 `arg(acp_subcommand)`（非空）。
      - `CmdFallback`：`cmd /C <bare_bin> <subcommand>`（现状）。
      - `NotInstalled { hint }`：返回 Err（错误串含 `npm i -g <hint>` 指引），**不 spawn**。
- [ ] `env_strip` 对前两态统一生效（现状保留）。
- [ ] **不引用任何引擎名**（AC8）：分支只按 `SpawnSpec`/`LaunchKind`。
- [ ] 尽量把 argv 构造抽纯函数以便单测。

**验证**：`cargo check` + `cargo test --lib`。
**回滚点**：`acp.rs`。

## Step 5 · Rust：检测引擎化（`agent.rs`）

- [ ] `detect_inner`：`bin_path` = PATH 命中 **或** npm 全局 bin 命中（ExternalAdapter）。
- [ ] `detect_args` 为空 → 跳过版本探测（version=None，不报问题）。
- [ ] `installed` 判据 = resolve 到命令。
- [ ] 未装适配器时的 `detail` 文案明确（如「未检测到适配器命令，请运行 `npm i -g <package>`」）。
- [ ] `config_path`：claude/codex 返回 None（D-cfg）。
- [ ] 单测（可注入路径的纯函数部分）。

**验证**：`cargo check` + `cargo test --lib`。
**回滚点**：`agent.rs`。

## Step 6 · Rust：超时矩阵与命令面复核（`acp.rs`）

- [ ] 若 Step 0 实测首连握手 > 30s，放宽 `timeout_for`（`session/new`/`initialize`）
      或在 `connect_inner` 首连给更长预算（**加引擎维或全局**，注释说明）。
- [ ] 复核 `acp_list_agents` / `acp_connect` / `acp_probe_options` /
      `agent_model_context_window` 对新变体自动兼容（无需改，确认）。

**验证**：`cargo check`。
**回滚点**：`acp.rs`。

## Step 7 · 前端：类型 + 兜底列表

- [ ] `src/types/engine.ts`：`EngineWire` → `'open_code' | 'claude_code' | 'codex'`；
      `BUILTIN_ENGINE_WIRES` 补 2 项。
- [ ] `engineService.mockList` 补 claude/codex 两项（浏览器 dev 可见）。
- [ ] `stores/engine.ts` 静态兜底补 2 项。

**验证**：`pnpm vue-tsc --noEmit`。
**回滚点**：三个前端文件。

## Step 8 · 前端：文案 + 图标 + 设置页

- [ ] `useConversation.ts`：`authHintFor` / `engineDisplayName` 补两引擎。
- [ ] `MessageComposer` agentName 兜底核对（按会话引擎）。
- [ ] `AgentManageView.vue` / `NewSessionDialog.vue` 图标映射补 claude/codex；
      `AgentManageView` 详情对非 opencode 走通用占位（PrecheckPanel 展示检测结果）。
- [ ] 文案：新建弹窗/设置页说明「经官方适配器启动，需自行安装厂商 CLI 与适配器
      （`npm i -g <package>`）」——**不承诺自动下载**。

**验证**：`pnpm vue-tsc --noEmit` + `pnpm build`。
**回滚点**：前端视图文件。

## Step 9 · 全量质量检查（trellis-check）

- [ ] `cargo check`；`cargo test --lib`；`pnpm vue-tsc --noEmit`；`pnpm build`。
- [ ] AC8 自查：`rg -n "claude|codex|ClaudeCode|Codex" src-tauri/src/acp.rs` →
      协议路由函数（reader_loop/handle_incoming/handle_client_request 等）**无引擎名**；
      仅 spawn 构造/超时/注册表引用。
- [ ] AC11 自查：全仓无打包适配器、无 `npm install`/`npx` 下载调用（仅解析已装命令）。
- [ ] 跨层核对：`EngineWire` 三处（Rust as_wire / TS union / mock）值一致。
- [ ] 旧数据兼容：无 engineId 的 index 仍回退 `open_code`。

## Step 10 · 实机验收（Tauri，人工）

- [ ] opencode 全流程同改造前（AC6）。
- [ ] Claude 会话：建连 → prompt → 流式 → 工具面板 → 停止 → 权限 → 模型下拉（AC5）。
- [ ] Codex 会话：同上（AC5）。
- [ ] 未安装适配器场景：检测 unavailable + 安装指引 + 弹窗 fallback（AC9）。
- [ ] 浏览器 mock：3 引擎 + 全流程（AC7）。
- [ ] 把实测发现回填 `research/` 与（如适用）spec。

---

## 关键契约（不可破坏）

- **wire 名**：`open_code` / `claude_code` / `codex`（持久化契约，单测锁）。
- **协议路由不含引擎名**（AC8）：引擎差异只在 `AcpAgentMeta.launch`（数据）+ `SpawnSpec`（3 态）。
- **opencode 零回归**：`OPENCODE_META` 行为与改造前逐字节一致。
- **命名空间**：仅 `~/.lightr-vibe/lightr-vibe/`。

## 依赖顺序（写进本文件，非树位置）

Step 0 → Step 1 → Step 2 → Step 3 → Step 4 → Step 5 → Step 6 → Step 7 → Step 8
→ Step 9 → Step 10。Step 1/2 可并行编辑但 Step 3 依赖 Step 2；前端 Step 7/8 依赖
Step 1（wire 联合类型）。
