# codeg 多-agent 抽象参考（只读研究）

> 来源：`H:\AI\Light-vibe\codeg`（参考项目，只读）。本文件提炼**照抄什么、砍掉什么**，
> 供 implement/check 子代理核对。架构事实以 codeg 源码为准，非猜测。

## 1. 身份层：AgentType 编译期枚举（照抄）

`src-tauri/src/models/agent.rs`
- `enum AgentType`（`:23`）：15 个内置变体 + `Custom(&'static str)`；`#[derive(Copy, Hash, Eq…)]`。
- `as_wire()`（`:93`）：稳定 wire/持久化表示（内置 snake_case，如 OpenCode→`"open_code"`；
  自定义→`"custom:<id>"`）。**wire 名是持久化契约**。
- `from_wire()`（`:116`）：反向解析；未知内置名/坏 slug → `None`。
- 自定义 `Serialize`/`Deserialize`（`:178`,`:184`）：走 wire；未知报 `D::Error::custom`。
- `BUILTIN_AGENT_TYPES`（`:47`）：所有内置变体数组。
- 测试 `builtin_wire_names_are_unchanged`（`:226`）：**锁死 wire 名**——注释明言
  "changing one silently orphans rows"。
- `is_valid_custom_agent_id`（`:147`）：slug 校验（防路径穿越/遮蔽内置）——**本期不做**。

**我们抄**：枚举 + `as_wire`/`from_wire` + `BUILTIN_AGENT_TYPES` + wire 不变单测。
**我们砍**：`Custom` 变体、intern、slug 校验、custom_registry。

## 2. 元数据层：静态注册表（照抄结构，裁剪字段）

`src-tauri/src/acp/registry.rs`
- `AcpAgentMeta`（`:130`）：`{ agent_type, supports_mcp, name, description, distribution }`。
- `AgentDistribution`（`:4`）：`Npx{version,package,cmd,args,env,node_required}` |
  `Binary{version,cmd,args,env,platforms,dir_entry}` | `Uvx{...}` —— **含下载/多平台/校验**。
- `get_agent_meta(agent_type)`（`:512`）：infallible `match` 穷尽查表。
- `registry_id_for`/`from_registry_id`（`:260`,`:282`）：注册表 id ↔ AgentType。
- opencode 条目（`:3228`）：`cmd:"opencode", args:["acp"]`，binary 分发 + 6 平台 URL。

**我们抄**：`AcpAgentMeta` + `get_agent_meta` 的 `match` 穷尽查表思路；`AcpAgentInfo` 形状。
**我们砍**：`AgentDistribution` 的下载/安装/多平台/`dir_entry`；只保留「怎么 spawn 系统已装引擎」。

## 3. spawn 层：build_agent（照结构，警惕分支）

`src-tauri/src/acp/connection.rs::build_agent`（`:1861`）
- `match meta.distribution` → 按分发构造命令行。
- **大量 `if agent_type == AgentType::X` 专属分支**（Grok `--permission-mode`、OpenClaw
  `--url`、Codex env policy、Pi preflight/trust…）。
- ⚠️ **反例警示**：这些内联分支是代码膨胀源。我们 D6：能声明化的一律入 `AcpAgentMeta`。

**我们抄**：单一 spawn 构造入口 + 按 meta 取参。
**我们避免**：海量 `if agent_type == …`。

## 4. 连接层：manager（结构参考）

`src-tauri/src/acp/manager.rs`
- `ConnectionManager`：连接带 `agent_type`（`:288`）；`find_connection_for_reuse`
  （按 agent_type+cwd+session 复用，`:734`）；per-agent_type 探测锁（`:459`）；
  `disconnect_by_agent_type`。
- 我们已有 per-session `CONNS<connId, AcpConn>`（用户定：不跨会话复用），仅需给
  `AcpConn` 加 `engine: AgentType`。

## 5. 前端：单一共享 store + 选择器（照抄）

`src/hooks/use-acp-agents.ts`
- zustand 单一共享 store；`acpListAgents()` 拉取；按 `sort_order`+`name` 排序（`:59`）；
  ref-counted 共享订阅；**失败保留上一次列表**（`:75` catch 注释）。
- `AcpAgentInfo { agent_type, name, enabled, available, installed_version, sort_order,
  icon_url, … }`（`:28`）。

`src/components/chat/agent-selector.tsx`
- 横向 pill 选择器；默认选择逻辑（`:93`）：`defaultAgentType` 可用→用，否则首个 available。
- `onFallback`（`:51`）：区分**系统兜底**与**用户确认**——fallback 不回写默认。

`src-tauri/src/acp/types.rs::AcpAgentInfo`（`:1504`）：列表下发形状的权威定义。

## 6. 设置/持久化（裁剪）

`src-tauri/src/db/entities/agent_setting.rs`：每 agent 一行
`{ id, agent_type, registry_id, enabled, sort_order, installed_version, env_json,
model_provider_id, … }`。

**我们抄**：注册表条目预留 `enabled`/`sort_order` 字段（为将来铺路）。
**我们砍**：DB 表——本期用一个 `settings.json` 存默认引擎。

## 7. 抄/砍清单（决策表）

| 维度 | codeg | 本期 | 理由 |
|---|---|---|---|
| 身份 | `AgentType` 枚举 + wire | ✅ 照抄 | 编译期安全 + 持久化契约 |
| 元数据 | `AcpAgentMeta` + `get_agent_meta` | ✅ 照抄结构 | 数据驱动优于分支 |
| 分发 | Npx/Binary/Uvx 下载安装 | ❌ 砍 | 只 spawn 系统已装引擎 |
| 自定义引擎 | `Custom` + DB 注册 | ❌ 砍 | 本期不做 |
| spawn | `build_agent` match + 分支 | ⚠️ 抄结构忌分支 | 避免膨胀 |
| 连接 | ConnectionManager 复用 | ⚠️ 已有 per-session | 用户定不复用 |
| 前端 | 单一共享 store + 选择器 fallback | ✅ 照抄 | 稳定 + 不误写默认 |
| 设置 | agent_setting DB 表 | ⚠️ 裁剪为 settings.json | 轻量 |
