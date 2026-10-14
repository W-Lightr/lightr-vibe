# 研究：codeg 如何启动 Claude Code / Codex 适配器（只读）

> 来源：`H:\AI\Light-vibe\codeg`（只读参考）。本文件只提炼**本期需要的启动事实**，
> 供 design/implement/check 子代理核对，非猜测。

## 1. 核心事实：原生 CLI 不说 ACP

`registry.rs:308-361`（`AcpAdapterRelation` 注释）明确：

- **Claude Code、Codex 是唯二例外**：`claude` 和 `codex` 原生 CLI **都不说 ACP**。
- codeg 安装一个**独立适配器包**（由 Agent Client Protocol 组织维护）：
  - Claude → `@agentclientprotocol/claude-agent-acp`
  - Codex → `@agentclientprotocol/codex-acp`
- 适配器的命令名与厂商 CLI 名**无关**（`claude-agent-acp` / `codex-acp`），这是用户困惑的头号来源。
- 适配器与厂商 CLI **共享配置/凭据目录**（`~/.claude` / `~/.codex`），装适配器无需二次登录。
- 厂商 CLI 的安装目录 `~/.local/bin`（claude）等，GUI 应用 PATH 常看不到，需额外探测。

opencode 不是适配器：它是官方自带 `opencode acp` 子命令（`registry.rs:3249-3287`）。

## 2. 分发形态：AgentDistribution（`registry.rs:4-59`）

```rust
pub enum AgentDistribution {
    Npx { version, package, cmd, args, env, node_required },     // npx 拉取
    Binary { version, cmd, args, env, platforms, dir_entry },    // 下载二进制（本期砍）
    Uvx { version, package, cmd, args, env, uv_required, python, system_cmd }, // uvx（本期砍）
}
```

- **Claude Code**（`registry.rs:2060-2068`）：
  ```rust
  Npx {
    version: "0.89.1",
    package: "@agentclientprotocol/claude-agent-acp@0.89.1",
    cmd: "claude-agent-acp",
    args: &[],
    env: &[],
    node_required: Some("22.0.0"),
  }
  ```
- **Codex**（`registry.rs:3062-3069`）：
  ```rust
  Npx {
    version: "2.2.2",
    package: "@agentclientprotocol/codex-acp@2.2.2",
    cmd: "codex-acp",
    args: &[],
    env: &[],
    node_required: Some("20.3.0"),
  }
  ```
- **OpenCode**（`registry.rs:3249-3287`）：`Binary { cmd:"opencode", args:["acp"], ... }`。

## 3. spawn 层：build_agent（`connection.rs:1861-2100`）

Npx 分支（`:1884-2006`）流程：

1. `merge_agent_env(env, runtime_env, scratch)` 合并声明 env + 运行时 env。
2. 大量 `if agent_type == X` 专属分支（Pi/Grok/OpenClaw/Codex）——**我们 D6 明确避免**。
3. `resolve_npx_command(cmd)`（`commands/acp.rs:413`）解析命令真身路径：
   - 先 `resolve_system_agent_binary(cmd)`（PATH 上直接找 `claude-agent-acp` / `codex-acp`）。
   - 失败再 `resolve_npx_command_from_current_npm_prefix`：跑 `npm prefix -g` 拿全局前缀，
     在 `<prefix>/bin`（Windows 在 `<prefix>`）下找 `cmd.cmd` / `cmd.exe` / `cmd`。
   - 即：**先找 PATH 上已装的适配器命令，再找 npm 全局装的，找不到才 fallback**。
4. `parts` 拼成 argv，交给 `AcpAgent::from_args`。

关键点：**codeg 并不是每次跑 `npx <pkg>`**，而是先解析「适配器命令真身」到 PATH 或 npm 全局
bin；只有 `npx` 作为**分发/安装手段**（首次用时 npx 会下载到 npm 缓存）。启动时优先复用已装命令。

## 4. codex 专属 env 策略（`connection.rs:1007-1040`）

`apply_codex_env_policy`（仅 Codex）：
- 强制 `DISABLE_MCP_CONFIG_FILTERING=true`（保证注入的 MCP 服务器不被过滤掉）。
- `INITIAL_AGENT_MODE`（从 `~/.codex/config.toml` 推断）——**本期不需要**（我们没有 MCP 注入，
  也不做 codex 模式推断；如需要后续单独任务）。

**本期判断**：我们不做 MCP 注入，也不需要 codex 模式推断 → **不需要 codex 专属 env 分支**。
若实测 codex-acp 因 MCP 过滤行为异常，再单独评估。

## 5. 能力/协议差异（本期最大的风险面）

codeg 在 registry 注释里记录了海量 claude/codex 专属能力协商（steering/goal/AIR
sessionFailure/elicitation/plan/permission changes/contextCompaction…）。这些**全部是本项目
Out of Scope**：

- 我们已有 ACP 消费面（`acpService` / `useConversation` / `acpEvents`）只处理
  `session/update` 的通用内容块 + `session/request_permission` + `configOptions`。
- claude/codex 的 `session/new` 的 `configOptions`（模型列表）走**标准 ACP 路径**，
  与我们现有 opencode 消费面一致 → 模型下拉理论上可直接复用。
- 若某引擎不返回 configOptions（或返回形状不同），前端已有 null 容忍；模型下拉退化为空。
- **本期不实现**任何引擎专属能力（steering/goal/AIR/elicitation/plan 等）。

## 6. 检测（acp_ok / version）

codeg 对适配器引擎的检测特殊点（`AcpAdapterRelation`）：
- 探测的是**厂商 CLI**（`claude` / `codex`）而非适配器命令；
- 且 preflight/diagnostics 会解释「适配器 vs 厂商 CLI」的拆分。

**本期简化**：检测适配器命令本身（PATH 或 npm 全局 bin 下的 `claude-agent-acp` /
`codex-acp`），是否支持 `--version`/`--help` 见 §7 实测。不探测厂商 CLI（用户自装，适配器
自己会调；凭据问题在 prompt 时以 auth 错误暴露）。

## 7. Step 0 实测结果（已完成，2026-10-13）

> **前提（已定）**：照 codeg，**用户自装**厂商 CLI + 适配器；本体不打包、不自动下载。
> 实测方式：本地 prefix 装（`H:\...\.acp-adapters`，因 D 盘满）+ 手写 JSON-RPC 探针
> （`initialize` → `session/new`）。

**结论（定稿字段值）：**

1. **`--version` 两者都支持**：
   - `claude-agent-acp --version` → `0.89.1`（退出码 0）。
   - `codex-acp --version` → `@agentclientprotocol/codex-acp 2.2.2`（退出码 0）。
   - → `detect_args = ["--version"]`（**两引擎都用**）。
2. **`--help` 不可靠**：`claude-agent-acp --help` 退出 0 但无输出；`codex-acp --help`
   退出码 `-1073740791`（0xC0000409，崩溃）。
   - → **`acp_probe_args = ["--version"]`**（用 version 当 ACP 可用探针），**不用 `--help`**。
     更稳：`installed` 判据 = resolve 到命令（D5），探测 best-effort 不阻塞。
3. **首连握手耗时**：`initialize` ~1.0–1.5s；`session/new` claude **~5.4s** / codex **~1.7s**。
   - → 现有超时矩阵（initialize 10s / session/new 30s）**足够，无需上调**。
4. **`configOptions` 形状与 opencode 一致**（标准数组）：
   - claude → `array[3]`（mode/…）；codex → `array[5]`（mode/…）。
   - → 前端模型/模式下拉**直接复用**现有消费面，无需改。
   - agentInfo：claude `{name:"@agentclientprotocol/claude-agent-acp", title:"Claude Agent",
     version:"0.89.1"}`；codex `{name:"@agentclientprotocol/codex-acp", title:"Codex",
     version:"2.2.2"}`。
5. **Windows shim**：`npm i` 产出的 bin 为 `claude-agent-acp.cmd` / `.ps1`（无裸名/无 .exe）。
   - → **必须走既有 `.cmd` shim 处理**（`extract_exe_from_shim` / `cmd /C` 回退）。
     bin_candidates 需含 `.cmd`；裸名候选在 Windows 不存在。
6. **空 env_strip**：两者无已知需清洗的环境变量（`env_strip = []`）。
7. **厂商 CLI 不需要在 PATH**：适配器**内嵌自带**厂商 CLI——
   - claude：`node_modules/@anthropic-ai/claude-agent-sdk-win32-x64/claude.exe`；
   - codex：`node_modules/@openai/codex/vendor/x86_64-pc-windows-msvc/bin/codex.exe`。
   - → **不探测厂商 CLI**；凭据（登录）问题在 prompt 时以 auth 错误暴露。
   - ⚠️ **实测踩坑**：codex 的平台二进制是 npm **optional dependency**
     （别名 `@openai/codex-win32-x64` → `@openai/codex@0.160.1-win32-x64`）。本机 npm
     配置**跳过了 optional 平台包**，导致 `codex.exe` 缺失/损坏（截断），适配器报
     `Missing optional dependency @openai/codex-win32-x64` / `spawn EFTYPE`。
     **用户若遇此问题**：`npm i -g @openai/codex@latest` 重装 codex（其 error 文案即这么说）。
     这是 **用户环境问题**，不是我们代码的问题；检测到 should 给出可读提示。
8. context-window：本期返回 `None`（前端保持既有分母），非阻塞。

**下一步（写进 implement.md）**：字段值已定稿 → 直接按 design §3.5 实现。

## 7b. 体积实测（决定"不打包"）

- `claude-agent-acp@0.89.1`：装完 **287MB**（`@anthropic-ai/claude-agent-sdk` 占 262MB）。
- `codex-acp@2.2.2`：`@openai/codex` 的 `codex.exe` 单文件 **326MB**。
- 两引擎合计 **~600MB**（+codex voice 资源）。
→ 适配器并非轻量桥接器，**打包自带不现实**。与 codeg 一致：只解析用户已装命令。

## 8. 本项目现状（引擎化地基已就绪）

- `agent_type.rs`：`AgentType` 枚举 + `as_wire`/`from_wire`（wire `"open_code"`）。**新增变体即可**。
- `engine.rs`：`AcpAgentMeta` + `LaunchSpec`（bin_candidates/acp_subcommand/env_strip/detect_args/
  acp_probe_args/config_path/context_window）+ `get_agent_meta` match 穷尽 + `resolve_spawn`。
- `acp.rs`：`build_acp_command(meta)` 按 meta spawn；`AcpConn.engine` 已绑定；`acp_connect(sessionId, engineId)`。
- `agent.rs`：`agent_detect(engineId)` / `agent_read/write_config(engineId)` 已引擎化。
- 前端：`engineService` / `stores/engine.ts` / `NewSessionDialog` / `settings.json` 默认引擎已就绪。

**结论**：本期=给两枚新枚举变体补 `AcpAgentMeta` 落点，并扩展 `LaunchSpec` 支持
「外部适配器命令」这一新分发形态（当前 `LaunchSpec` 只有 bin_candidates + 单一 acp_subcommand，
不足以表达「命令真身可能来自 npm 全局 bin」）。**照 codeg：用户自装，我们只解析，不打包、不下载。**
