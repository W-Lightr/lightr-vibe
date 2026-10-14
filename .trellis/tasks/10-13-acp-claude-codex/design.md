# Design: 接入 Claude Code 与 Codex ACP 适配器引擎

需求见 `prd.md`。研究事实见 `research/codeg-claude-codex.md`。
桥接规范：`.trellis/spec/frontend/tauri-bridge.md`。地基见已归档 `10-13-acp-multi-engine`。

## 1. 架构总览

本期**不新增文件层**，只在既有引擎注册表上做三点扩展：

```
Rust  src-tauri/src/agent_type.rs    + ClaudeCode / Codex 变体 + wire 名
      src-tauri/src/engine.rs        + LaunchKind（分发形态：DirectCommand | ExternalAdapter）
                                      + npm 全局 bin 解析 + 两条 meta 臂
      src-tauri/src/acp.rs            build_acp_command 按 LaunchKind 构造（无引擎名分支）
      src-tauri/src/agent.rs          detect 按 LaunchKind（best-effort 探测）
前端  src/types/engine.ts             EngineWire 联合扩 3 项
      src/services/engineService.ts   mock 兜底列表补 2 项
      src/stores/engine.ts            静态兜底列表补 2 项
      src/composables/useConversation.ts  authHint/displayName 补 2 引擎
      src/views/.../AgentManageView.vue + NewSessionDialog.vue  图标映射补 2 引擎
```

**核心难点**：`LaunchSpec` 当前只能表达「PATH 上一个 bin + 一个静态 subcommand」
（opencode 形态）。claude/codex 需要「命令真身可能不在 PATH，而在 npm 全局 bin」
（用户 `npm i -g` 装的适配器命令）。→ 引入 `LaunchKind` 枚举 + npm 全局 bin 解析。
**不打包、不自动下载**（照 codeg：用户自装 CLI + 适配器）。

## 2. Rust：AgentType 扩展（`agent_type.rs`）

```rust
pub enum AgentType {
    OpenCode,
    ClaudeCode,   // 新
    Codex,        // 新
}

pub const BUILTIN_AGENT_TYPES: &[AgentType] =
    &[AgentType::OpenCode, AgentType::ClaudeCode, AgentType::Codex];

impl AgentType {
    pub fn as_wire(&self) -> &'static str {
        match self {
            AgentType::OpenCode   => "open_code",
            AgentType::ClaudeCode => "claude_code", // 照 codeg models/agent.rs:95
            AgentType::Codex      => "codex",        // 照 codeg models/agent.rs:96
        }
    }
    // from_wire: 反向三臂；未知名 → None
}
```

- **wire 名是持久化契约**（`sessions-index.json` 的 `engineId`）；沿用 codeg 既有字符串，
  保证语义一致、将来数据互通无歧义。加单测锁死三对映射。
- `Display` 走 `as_wire`（现状）。

## 3. Rust：LaunchSpec 分发形态扩展（`engine.rs`）

### 3.1 现状与缺口

现有 `LaunchSpec`：`bin_candidates` + `acp_subcommand` + `env_strip` + `detect_args` +
`acp_probe_args` + `config_path` + `context_window`。`resolve_spawn` 只做
「PATH 扫 bin_candidates → .cmd shim 提取 → cmd /C 回退」。

**缺口**：claude/codex 的命令真身（`claude-agent-acp` / `codex-acp`）可能只存在于
**npm 全局 bin**（用户 `npm i -g` 装的），不在 PATH。**照 codeg：用户自装 CLI + 适配器，
我们只解析（PATH → npm 全局 bin），不打包、不自动下载。**

> 体积实测（research §2 增补）：适配器**不是轻量桥接器**——`claude-agent-acp` 依赖
> `@anthropic-ai/claude-agent-sdk`（装完 ~287MB，其中 262MB 是 SDK），`codex-acp` 依赖
> `@openai/codex`。故 codeg 与我们都**不打包**；解析到就用，解析不到报错+安装指引。

### 3.2 设计：新增 `LaunchKind`

```rust
/// 引擎的分发形态（照 codeg AgentDistribution 裁剪，仅保留 launch 必需两态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchKind {
    /// 直接启动 PATH 上的可执行/子命令（opencode：`opencode acp`）。
    /// 语义同现状：resolve_spawn(meta) 走 bin_candidates + shim 回退。
    DirectCommand,
    /// 外部适配器命令（用户自装）：命令真身在 PATH 或 npm 全局 bin。
    /// spawn 时：PATH 命令 → npm 全局 bin 命令；全未命中 → 报「未安装适配器」。
    ExternalAdapter,
}
```

`LaunchSpec` 增加两个字段：

```rust
pub struct LaunchSpec {
    pub bin_candidates: &'static [&'static str],
    pub acp_subcommand: &'static str,
    pub env_strip: &'static [&'static str],
    pub detect_args: &'static [&'static str],
    pub acp_probe_args: &'static [&'static str],
    pub config_path: Option<&'static [&'static str]>,
    pub context_window: ContextWindowSource,
    // ---- 新增 ----
    pub kind: LaunchKind,
    /// ExternalAdapter 专用：npm 包规格（如 "@agentclientprotocol/claude-agent-acp"），
    /// **仅用于错误提示里的安装命令指引**（`npm i -g <package>`），不触发下载。
    /// DirectCommand 时为 None。
    pub install_hint: Option<&'static str>,
}
```

- **`acp_subcommand` 语义**：DirectCommand 下为子命令（`"acp"`）；ExternalAdapter 下为 `""`
  （适配器命令本身就是 ACP 入口，无子命令）。构造 argv 时对空串跳过。
- **opencode 条目**：`kind: DirectCommand`，`install_hint: None`（回归零变化）。

### 3.3 新增：npm 全局 bin 解析（`engine.rs` 内，纯 std）

```rust
/// 解析 npm 全局 bin 目录：跑 `npm prefix -g` 取前缀 → `<prefix>`（Windows）
/// 或 `<prefix>/bin`（类 Unix）。带超时（复用 agent.rs 的 5s 模式或独立短超时）。
/// 失败返回 None（不报错，解析链继续/最终报未安装）。
fn resolve_npm_global_bin_dir() -> Option<PathBuf>;

/// 在 npm 全局 bin 目录按 bin_candidates 找适配器命令真身（.cmd/.exe/裸名）。
fn resolve_bin_in_npm_dir(meta: &AcpAgentMeta) -> Option<String>;
```

> 与 codeg `resolve_npx_command_from_npm_prefix`（`commands/acp.rs:509`）同思路。

### 3.4 spawn 解析链扩展

现有 `resolve_spawn(meta) -> SpawnSpec` 只处理 PATH。扩展为：

```
match meta.launch.kind {
  DirectCommand => 现状链（PATH bin_candidates → shim → cmd /C），不变，
                   命令后追加 meta.launch.acp_subcommand（非空时）
  ExternalAdapter => 1) PATH 扫 bin_candidates（同 resolve_bin）
                     2) 命中 → 走既有 shim/cmd 回退构造 spawn（无 subcommand）
                     3) 未命中 → resolve_bin_in_npm_dir(meta)
                        命中 → SpawnSpec::Exe(该路径)（含 .cmd shim 提取）
                     4) 全未命中 → SpawnSpec::NotInstalled { hint }  ← 报错
}
```

`SpawnSpec` 增加一态：

```rust
pub enum SpawnSpec {
    Exe(PathBuf),
    CmdFallback,                 // DirectCommand 的 cmd /C 回退（现状）
    /// ExternalAdapter 全未命中：适配器未安装，携带安装指引，spawn 时报错。
    NotInstalled { hint: String },
}
```

`acp.rs::build_acp_command` 按 `SpawnSpec` 构造：
- `Exe(p)`：`Command::new(p)` + （DirectCommand 时）`arg(acp_subcommand)`。
- `CmdFallback`：`cmd /C <bare_bin> <subcommand>`（现状）。
- `NotInstalled { hint }`：返回 `Err`（错误串 `acp:-32603 未安装适配器：...` + hint），
  **不 spawn 任何进程**。`acp_connect` 的 `spawn().map_err(...)` 路径改为先判 `NotInstalled`。
  `env_strip` 逻辑对前两态不变。

**为什么这么设计**：把「引擎差异」全部压进 `AcpAgentMeta.launch`（数据），
`build_acp_command` 只对 `LaunchKind`/`SpawnSpec`（3 个通用形态）分支，**不出现引擎名**（AC8）。
这是 D6（避免 codeg 的 `if agent_type == …` 海量分支）的落实。

### 3.5 claude / codex 的 `AcpAgentMeta`

```rust
const CLAUDE_CODE_META: AcpAgentMeta = AcpAgentMeta {
    agent_type: AgentType::ClaudeCode,
    name: "Claude Code",
    description: "Anthropic Claude 的 ACP 适配器（用户自装 claude-agent-acp）",
    launch: LaunchSpec {
        bin_candidates: if cfg!(windows) {
            &["claude-agent-acp.exe", "claude-agent-acp.cmd", "claude-agent-acp.bat", "claude-agent-acp"]
        } else { &["claude-agent-acp"] },
        acp_subcommand: "",                  // 适配器命令即 ACP 入口
        env_strip: &[],                      // 实测：无已知需清洗变量
        detect_args: &["--version"],         // 实测：支持（0.89.1）
        acp_probe_args: &["--version"],      // 实测：--help 不可靠，用 --version 当探针
        config_path: None,                   // D-cfg：无独立可编辑配置文件（见 §8）
        context_window: ContextWindowSource::None,
        kind: LaunchKind::ExternalAdapter,
        install_hint: Some("@agentclientprotocol/claude-agent-acp"),
    },
};
// CODEX_META 同形：bin "codex-acp*"，config_path None，
// detect_args/acp_probe_args 同为 ["--version"]（实测 2.2.2），
// install_hint "@agentclientprotocol/codex-acp"
```

- **不 pin 版本**：用户自装什么版本就用什么（我们不再控制版本）。`install_hint` 只用于
  错误提示里的 `npm i -g <package>` 指引，不含版本号（避免误导）。
- **`config_path: None`**（D-cfg）：claude/codex 无**独立可编辑**配置文件（凭据在厂商
  CLI 的 `~/.claude`/`~/.codex`，由 CLI 自己管），设置页不提供配置编辑。
- **厂商 CLI 无需在 PATH**（实测）：适配器内嵌自带厂商 CLI（claude-agent-sdk 的 claude.exe /
  `@openai/codex` 的 codex.exe）。凭据问题在 prompt 时以 auth 错误暴露。
- **codex optional dep 坑**（实测）：`@openai/codex` 的平台二进制是 npm optional dependency；
  部分 npm 配置会跳过它 → 用户需 `npm i -g @openai/codex@latest` 重装。这是**用户环境问题**，
  我们仅在检测/错误文案里给可读提示（见 §9 风险）。

## 4. Rust：检测与命令面

### 4.1 检测（`agent.rs::detect_inner`）

现状：`resolve_bin`（PATH）+ `--version` + `acp --help`。

扩展：
- `bin_path` = PATH 命中 **或** npm 全局 bin 命中（ExternalAdapter）；`installed` 判据 =
  resolve 到命令（D5）。
- `detect_args` 为空时跳过版本探测（避免无意义超时），version = None。
- `acp_probe_args` 视实测决定；探测失败不改变 `installed`（仅 `acp_ok=false`）。
- **不做任何下载**；两处都未 resolve → `installed=false`，`detail` 给出安装指引
  （`npm i -g <install_hint>`）。

### 4.2 命令面

- `acp_list_agents`：`BUILTIN_AGENT_TYPES` 驱动，自动 3 项；无需改。
- `acp_connect` / `acp_probe_options` / `agent_model_context_window` / `agent_detect`：
  已参数化，新增变体自动兼容。
- `acp_connect` 新增 `SpawnSpec::NotInstalled` 处理：解析不到适配器时返回明确错误串
  （含安装指引），不 spawn。
- `agent_read/write_config`：claude/codex 的 `config_path` 为 `None` → 返回空/报「该引擎
  无独立配置文件」，设置页走通用占位（D-cfg）。

## 5. 前端

### 5.1 类型与兜底列表
- `src/types/engine.ts`：`EngineWire` 联合 → `'open_code' | 'claude_code' | 'codex'`；
  `BUILTIN_ENGINE_WIRES` 补 2 项。
- `engineService.mockList` / `stores/engine.ts` 静态兜底：补 Claude/Codex 两项
  （浏览器 dev 无 Tauri 时可见可选）。

### 5.2 显示名/文案
- `useConversation.ts::authHintFor` / `engineDisplayName`：补
  `claude_code → 'Claude Code'`、`codex → 'Codex CLI'`；auth 文案走通用（适配器凭据由
  厂商 CLI 登录托管）或给针对性提示（待实测确定提示语）。
- `MessageComposer` agentName 兜底（上一任务已按会话引擎；核对新引擎）。

### 5.3 图标与设置页
- `AgentManageView.vue` / `NewSessionDialog.vue` 的 `open_code: opencodeIcon` 映射：
  补 claude/codex 图标（`@lobehub/icons-static-svg` 已有 claude/codex）；无则用通用占位。
- `AgentManageView` 详情：`showOpencodeDetail` 之外新增 claude/codex 的通用详情占位
  （或复用 PrecheckPanel 显示检测结果，不做配置编辑）。

### 5.4 新建会话
- `NewSessionDialog` 引擎下拉自然列出 3 项（来自 `acp_list_agents`）。
- 默认预选 `settings.json` 的默认引擎；不可用 → fallback 首个 available（不写回默认）。

## 6. 持久化

- `Session.engineId` / `SessionIndexEntry.engineId` 已支持（上一任务）；新引擎值
  `"claude_code"` / `"codex"`；旧数据默认 `"open_code"`。
- `settings.json` 默认引擎校验：`engine_default_set` 已用 `AgentType::from_wire` 校验，
  新引擎自动合法。

## 7. 兼容 / 回滚

- 回滚 = 删两枚枚举变体 + 两条 meta 臂 + 前端兜底两项；`LaunchKind` 可保留（opencode
  走 DirectCommand 不变）。git 单任务 diff 整体 revert。
- 旧数据零影响（`open_code` 不变）。
- 新引擎未安装：检测 unavailable + 安装指引，建连报明确错误，不影响 opencode。

## 8. 决策点（已确认）

- **D1 用户自装**（用户确认）：不打包、不自带适配器；不自动下载。我们只解析
  PATH → npm 全局 bin。解析不到 → 报错 + `npm i -g <package>` 指引。
- **D-npx-ver**（作废）：因不控制版本，不 pin；`install_hint` 不含版本号。
- **D-detect**（收口）：检测只报「resolve 到适配器命令」为 available；未装 → unavailable +
  安装指引。无「未装但会自动下载」的双重语义。
- **D-cfg**：claude/codex 的 `config_path` 设 `None`（不做配置编辑），设置页走通用详情占位。
- **D-node**：不检测 Node/npm 版本下限；仅在解析不到时于错误文案里提示
  「需要 Node.js + `npm i -g <package>`」。

## 9. 风险

- **适配器不支持 `--version`/`--help`**：检测降级为 resolve-only（D5）。实现阶段先实测。
- **首连耗时**：适配器用已装命令（非 npx 下载），首连应为正常握手时间；仍实测确认
  （集成 SDK 的适配器启动可能比 opencode 慢）→ 决定是否为新引擎放宽 `timeout_for`。
- **Windows `.cmd` shim**：用户 `npm i -g` 的适配器命令常是 `.cmd`；复用既有
  `extract_exe_from_shim` / `resolve_spawn` 回退链（regression-safe）。
- **配置目录 vs 文件**：用 D-cfg 返回 None 规避。
- **协议消费面差异**：claude/codex 的 `session/update` 内容块形状若与 opencode 不同，
  前端 `acpEvents`/`useConversation` 可能不认 → 这是**最大未知**，须实机验证（AC5）。
- **凭据**：适配器共享 `~/.claude`/`~/.codex`；若未登录，prompt 会 auth 失败 →
  authHint 文案要针对新引擎。

## 10. 测试要点

- Rust 单测：
  - `AgentType` 三变体 wire 往返 + 未知拒绝 + `BUILTIN` 覆盖（照 codeg 测试风格）。
  - `get_agent_meta` 对每变体返回一致 `agent_type`（现有测试自动覆盖新增变体）。
  - `resolve_spawn`：DirectCommand 回归（opencode）；ExternalAdapter 的
    PATH 命中 / npm 命中 / 全未命中→NotInstalled 三态（npm 目录用注入路径的可测函数，
    不真跑 npm）。
  - `build_acp_command` argv 构造：三种 SpawnSpec（纯函数化以便测试）。
- 前端：3 引擎列表 mock；引擎选择链路（选择 → createSession(engineId) → 持久化）。
- 实机回归：opencode 全流程不变（AC6）；Claude/Codex 各建连 + prompt + 流式 + 工具 +
  权限 + 停止（AC5）。
