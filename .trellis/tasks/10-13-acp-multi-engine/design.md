# Design: 多引擎 ACP 抽象与新建会话选引擎

需求见 `prd.md`。协议契约：`../archive/2026-10/10-08-opencode-acp-chat/research/acp-protocol.md`。
桥接规范：`.trellis/spec/frontend/tauri-bridge.md`。基于已归档的 per-session 架构。

**架构来源：照 codeg 的多-agent 抽象裁剪**（参考实现路径见 `prd.md` Background）。
本期只保留 codeg 的三条主结构，砍掉其下载/安装/自定义/DB 重型机制。

## 1. 架构总览

```
Rust  src-tauri/src/agent_type.rs    新：AgentType 枚举 + wire 契约（照 codeg models/agent.rs）
      src-tauri/src/engine.rs        新：AcpAgentMeta 静态注册表 + get_agent_meta（照 codeg acp/registry.rs）
      src-tauri/src/acp.rs           spawn/probe/context-window 改为按 AgentType 取 meta
      src-tauri/src/agent.rs         检测/配置改为按 AgentType（detect_opencode → detect）
      src-tauri/src/lib.rs           命令注册（+ acp_list_agents / engine_default_*）
前端  src/services/engineService.ts  新：acp_list_agents + 默认引擎读写
      src/stores/engine.ts           新：单一共享 agent store（照 codeg use-acp-agents）
      src/stores/agent.ts            单 detect 结果 → 按 AgentType 键
      src/stores/session.ts          Session/SessionIndexEntry + engineId（wire）
      src/composables/useConversation.ts  MODEL_AGENT_KEY 常量 → 按 session.engineId
      src/views/coding/components/NewSessionDialog.vue  新：新建会话弹窗
      src/views/settings/components/AgentManageView.vue  注册表驱动 + 默认引擎
持久化 ~/.lightr-vibe/lightr-vibe/settings.json     新：全局设置（默认引擎）
       ~/.lightr-vibe/lightr-vibe/sessions-index.json + engineId（旧数据默认 opencode）
```

## 2. Rust AgentType 枚举（`agent_type.rs`，照 codeg `models/agent.rs`）

```rust
/// 会话/连接背后的引擎。本期仅一个内置变体；预留扩展。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentType {
    OpenCode,
}

pub const BUILTIN_AGENT_TYPES: &[AgentType] = &[AgentType::OpenCode];

impl AgentType {
    /// 稳定的 wire/持久化表示（写入 sessions-index.json 的 engineId）。
    /// 内置 wire 名是持久化契约，不可变（改名会孤立已存会话）。
    pub fn as_wire(&self) -> &'static str {
        match self { AgentType::OpenCode => "open_code" }
    }
    /// 解析 as_wire 的输出。未知 wire → None（上层报明确错误）。
    pub fn from_wire(s: &str) -> Option<Self> {
        match s { "open_code" => Some(AgentType::OpenCode), _ => None }
    }
}
// impl Display / Serialize / Deserialize(报错用 D::Error::custom)
```

- 照 codeg：`Serialize`/`Deserialize` 走 `as_wire`/`from_wire`；无 `Custom` 变体
  （本期不做用户自定义引擎，见 Out of Scope）。
- 单测：`as_wire`/`from_wire` 往返；未知 wire 拒绝；`BUILTIN_AGENT_TYPES` 覆盖所有变体。

## 3. Rust 引擎注册表（`engine.rs`，照 codeg `acp/registry.rs`）

```rust
/// 一个引擎的静态 launch 元数据。新增引擎 = 加枚举变体 + 一条 match 臂。
pub struct AcpAgentMeta {
    pub agent_type: AgentType,
    pub name: &'static str,          // 显示名 "OpenCode"
    pub description: &'static str,
    pub launch: LaunchSpec,
}

pub struct LaunchSpec {
    pub bin_candidates: &'static [&'static str], // resolve_bin 候选（含 .exe/.cmd/.bat）
    pub acp_subcommand: &'static str,            // "acp"
    pub env_strip: &'static [&'static str],      // spawn 时移除的环境变量
    pub detect_args: &'static [&'static str],    // ["--version"]
    pub acp_probe_args: &'static [&'static str], // ["acp","--help"]（acp_ok）
    pub config_path: Option<&'static str>,       // ~/.config/opencode/opencode.jsonc
    pub context_window: ContextWindowSource,     // opencode config+catalog | none
}

/// 照 codeg get_agent_meta：infallible 查表（枚举穷尽 match）。
pub fn get_agent_meta(agent_type: AgentType) -> AcpAgentMeta { ... }

/// 列表下发（照 codeg acp_list_agents_core 的 AcpAgentInfo 形状，裁剪）。
pub fn list_agents() -> Vec<AcpAgentInfo> { ... }

#[derive(Serialize)] #[serde(rename_all="camelCase")]
pub struct AcpAgentInfo {
    pub agent_type: AgentType,   // wire 序列化
    pub name: String,
    pub description: String,
    pub enabled: bool,           // 预留（本期恒 true）
    pub sort_order: i32,         // 预留
    pub available: bool,         // 检测通过
    pub installed_version: Option<String>,
}
```

- opencode 条目承载现状全部常量（`acp.rs:133-199`、`agent.rs:76,103,193,201`、
  `acp.rs:943-1079`）。
- **`agent.rs` 改造范围 = 整条 detect 链**（非仅 resolve_bin）：`resolve_bin`（`:100`）、
  `run_opencode`（`:192`，`Command::new("opencode")`）、`extract_version`（`:218`）、
  `detect_opencode_inner`（`:323`）、`opencode_config_path`（`:75`）全部按 meta 收引擎化。
- **`get_agent_meta` 用 `match agent_type` 穷尽枚举**（codeg 风格）。⚠️ **诚实标注**：
  本期仅一个变体时，`match` 只有一条臂，编译器穷尽性检查**无实质收益**；此设计的价值在
  「新增第 2 个引擎时编译器强制补臂」，属**架构就绪、非本期收益**。不得在 PR/文档中
  宣称为本期已获得的保证。
- **协议路由代码（reader_loop/handle_incoming/…）不引用引擎名**（AC8）；仅 spawn 构造、
  检测、配置路径、context-window 读 `AcpAgentMeta`。
- **警告**：codeg `build_agent` 里 `if agent_type == …` 分支会随引擎数膨胀。本期
  opencode 专属行为若无法声明化，才允许一个 `match`，且必须注释为何无法声明化。

## 4. 命令面改造

| 命令 | 变化 | 参数 |
|---|---|---|
| `acp_connect` | + engineId(wire) | `(sessionId, engineId)`；解析为 AgentType |
| `acp_probe_options` | + engineId | `(cwd?, engineId?)` |
| `agent_model_context_window` | + engineId | **现状签名 `(provider, model)`**（`acpService.ts:146`），改为 `(provider, model, engineId?)`；Rust 侧按 meta 取 context-window 策略 |
| `agent_detect_opencode` | 泛化 `agent_detect` | `(engineId)` |
| `agent_read/write_opencode_config` | 泛化 `agent_read_config(engineId)` / `write` | 路径来自 meta |
| `acp_list_agents` | 新（照 codeg） | `()` → `AcpAgentInfo[]` |
| `engine_default_get` / `set` | 新 | 读写 settings.json 的默认引擎（wire） |
| 会话级命令 | **不变**（仍 connId 定位） | 引擎自 `AcpConn.engine` 取 |

- `AcpConn` 增加 `engine: AgentType`（建连时写入）。
- lib.rs 注册同步更新；`agent_detect_opencode` 改名后前端同步（不做双注册长期兼容）。

## 5. 前端

- `stores/engine.ts`（照 codeg `use-acp-agents`）：**单一共享 store**；`agents`、
  `fresh`、`defaultEngineId`、`load()`、`refresh()`；按 `sortOrder`+`name` 排序；
  失败保留上一次列表（不因瞬时失败清空）；Tauri 缺席回退 `[{agentType:'open_code',
  name:'OpenCode'}]`。
- `NewSessionDialog.vue`：下拉选引擎，默认 `defaultEngineId`；不可用 → fallback 首个
  available（**不写回默认**，照 codeg `onFallback` 语义）→ 确认时把 `engineId` 传入
  `sessionStore.createSession`。ui-style 合规（neutral 卡 + rounded-md + text-[13px]）。
- `sessionStore.createSession(model, { engineId, ... })`：写 `session.engineId`；
  `toIndexEntry`/`sessionFromEntry` 读写该字段；缺省 `"open_code"`。
- `useConversation`：`MODEL_AGENT_KEY` 由常量改为 `session.engineId`；
  auth/门禁文案按引擎名。
- `AgentManageView`：`agents` 由 `stores/engine` 渲染；`selectedAgentId` 响应式；
  增加「默认引擎」选择写 settings。

## 6. 持久化

- 新文件 `~/.lightr-vibe/lightr-vibe/settings.json`（`{ defaultEngineId: "open_code" }`）；
  经 `engine_default_get/set` 读写（Rust 侧 std::fs，命名空间硬规则）。
- `sessions-index.json`：`SessionIndexEntry` + `engineId`（wire）；`parseIndex` 缺省
  `"open_code"`。
- `forkSession` 复制 `engineId`。

## 7. 决策

- **D1 枚举而非字符串**（照 codeg）：编译期穷尽 + wire 契约单测；取代裸字符串 id。
- **D2 静态 meta 注册表**（照 codeg `get_agent_meta`）：`match` 穷尽查表。
- **D3 引擎在 connect 时绑定 `AcpConn.engine`**，会话级命令不传引擎 → 命令面增量最小。
- **D4 context-window 归入 meta**：opencode 的 config→catalog 策略作该引擎声明的实现；
  无策略的引擎返回 `Ok(None)`。
- **D5 前端单一共享 store**（照 codeg use-acp-agents），失败不清空。
- **D6 砍掉 codeg 重型机制**：下载/安装/自定义/DB agent_setting（见 prd Out of Scope）。

## 8. 兼容/回滚

- 回滚 = 恢复 `acp.rs`/`agent.rs` 常量、删 `engine.rs`/`agent_type.rs`、前端删弹窗走
  单引擎（git 单任务 diff 整体 revert）。
- 旧会话数据（无 engineId）默认 open_code，行为与改造前一致（AC5）。
- **wire 契约**：`"open_code"` 一旦写入持久化即不可改（加单测锁）。

## 9. 风险

- **命令改名波及面**：`agent_detect_opencode` 被 `opencodeService`/`agent.ts` 引用，
  先 `rg` 全量定位再改。
- **前端启动顺序**：`acp_list_agents` 需在 NewSessionDialog 挂载前拉取；缓存进 store，
  失败回退静态 opencode。
- **mock 一致性**：`acpMock`/`opencodeMock` 补 `acp_list_agents`/`engine_default_*` 与
  detect 的 engineId 形状。
- **两处 resolve_bin 重复**（`acp.rs:130` / `agent.rs:100`）：收敛到 `engine.rs` 单一实现。
- **过度分支**：若 opencode 专属逻辑开始出现多个 `if agent_type`，说明该处应数据化。

## 10. 测试要点

- Rust 单测：`as_wire`/`from_wire` 往返 + 未知拒绝；`BUILTIN` 覆盖所有变体；
  `get_agent_meta` 对每变体返回一致 `agent_type`（照 codeg 测试）。
- 命令面参数化：`acp_connect(sessionId, "open_code")` 与省略默认等价。
- AC4 mock 引擎：临时加一枚举臂（仅测试/或纯前端 mock 校验选择链路）。
- 回归：实机 opencode 全流程同改造前（AC3）。
