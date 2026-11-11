# Design — ACP v1/v2 双栈与 claude-agent-acp 0.104 适配

## 1. 目标与边界

### 1.1 目标

在不改动前端 ACP 事件契约的前提下，让 `src-tauri/src/acp.rs` 同时支持：

- **ACP v1**：opencode、codex-acp、claude-agent-acp 0.90–0.102；
- **ACP v2**：claude-agent-acp 0.103+（本次验证 0.104.0）。

v2 的差异（握手字段、prompt 生命周期、update 变体、权限参数、配置项键名、
session 恢复方法）全部在 Rust 侧归一化，前端继续消费现有 v1 形状。

### 1.2 非目标

- 不实现 v2 的 terminal、session/fork、session/delete、后台任务等新 UI。
- 不实现完整的 `auth/login` / `auth/logout` 交互；只识别认证需求并给出可操作错误。
- 不修改 per-session 一进程一连接模型、`connId`/`sessionId` 路由、权限/提问事件名。
- 不按引擎名硬编码协议；协议只由握手结果决定（`LIGHTR_ACP_PROTOCOL` 仅作故障开关）。

## 2. 现状与根因

`AcpConn` 目前没有协议字段，`client_capabilities()` 固定返回 v1 形状，
`connect_inner` 与 `probe_options_inner` 固定发送：

```json
{ "protocolVersion": 1, "clientCapabilities": {...}, "clientInfo": {...} }
```

0.104.0 只服务 ACP v2，收到 v1 后返回 `-32022 Unsupported protocol version`。
同时 v2 的 prompt 响应只表示“用户消息已插入”，turn 结束在 idle `state_update`；
因此即使握手改通，现有 `rpc_call("session/prompt")` 也会提前返回，前端会在生成中途收尾。

## 3. 总体架构

```
┌─────────────────────────────── src-tauri/src/acp.rs ───────────────────────────────┐
│                                                                                    │
│  AcpConn { protocol: AtomicU8, prompt_waiters, message_text, ... }                  │
│                                                                                    │
│  connect_inner ──► initialize_connection ──► AcpProtocol::{V1,V2}                  │
│                         │                                                           │
│                         ├─ v2 initialize（capabilities/info）                       │
│                         ├─ 回退：同一连接重发 v1 initialize                         │
│                         └─ 失败：合并两次错误并 kill                                │
│                                                                                    │
│  session_new/load ──► session_method/params(protocol) ──► normalize configOptions   │
│  set_config       ──► set_config_params(protocol)                                  │
│  send_prompt      ──► V1: rpc_call(session/prompt)                                 │
│                       V2: waiter + ack + idle state_update                         │
│                                                                                    │
│  handle_incoming(session/update)                                                   │
│      ├─ state_update ──► 唤醒 prompt waiter（不 emit）                             │
│      └─ normalize_update(protocol) ──► 现有 acp://update 形状                       │
│                                                                                    │
│  handle_client_request(permission) ──► normalize_permission ──► acp://permission   │
└────────────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
                    前端 useConversation / PermissionCard 不变
```

## 4. 协议表示与协商

### 4.1 协议枚举

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AcpProtocol {
    V1,
    V2,
}

impl AcpProtocol {
    fn as_u8(self) -> u8 { match self { Self::V1 => 1, Self::V2 => 2 } }
    fn from_u8(v: u8) -> Option<Self> { match v { 1 => Some(Self::V1), 2 => Some(Self::V2), _ => None } }
    fn from_version(v: u64) -> Option<Self> { match v { 1 => Some(Self::V1), 2 => Some(Self::V2), _ => None } }
}
```

`AcpConn` 新增：

```rust
protocol: AtomicU8,                    // 0 = 未协商；1 = v1；2 = v2
prompt_waiters: Mutex<HashMap<String, Sender<PromptOutcome>>>, // key = ACP sessionId
message_text: Mutex<HashMap<String, String>>,                   // v2 整消息 upsert 去重
```

`fn protocol(&self) -> AcpProtocol` 读取 0 时回退 `V1`（初始化完成前不会走会话路径）。

### 4.2 环境开关

```rust
fn forced_protocol() -> Option<AcpProtocol> // LIGHTR_ACP_PROTOCOL=v1|v2|auto
fn parse_forced_protocol(raw: Option<&str>) -> Option<AcpProtocol> // 纯函数，便于单测
```

- `auto` / 未设置 / 非法值 → `None`（自动协商）。
- `v1` → 只发 v1，绝不尝试 v2。
- `v2` → 只发 v2，失败不回退（用于验证 0.104；也给高级用户强制开关）。

### 4.3 协商流程（有状态连接与 probe 共用语义）

1. 若允许 v2，发送 **v2 initialize**。
2. 响应 `protocolVersion == 2` → 确定 v2。
3. 响应 `protocolVersion == 1`（Agent 选择了较低的 v1）→ **同一连接重发 v1 initialize**
   （ACP v2 初始化文档允许 `initialize` 失败/降级后再次发送；用 v1 参数重新确认
   capabilities 与 info）。
4. 发送 v2 收到 `-32022` 或 `-32602` → 同一连接重发 v1 initialize。
5. v1 initialize 成功且 `protocolVersion == 1` → 确定 v1。
6. 其它错误（如 `-32000` 认证、进程退出、超时）→ 直接返回，不做 v1 重试。
7. 两次都失败 → 返回合并错误：

```text
acp:-32022 Unsupported protocol version; v1 fallback failed: acp-dead: ...
```

> 为什么允许同连接重发：ACP v2 初始化文档明确“initialize 失败后连接仍未初始化，
> Client MAY 再次发送 initialize”。这样 v1-only 适配器不需要重启进程，冷启动只付一次代价。

### 4.4 initialize 参数

```rust
fn client_capabilities(protocol: AcpProtocol) -> Value
fn initialize_params(protocol: AcpProtocol) -> Value
```

v1（保持现状，避免回归）：

```json
{
  "protocolVersion": 1,
  "clientCapabilities": {
    "fs": { "readTextFile": false, "writeTextFile": false },
    "elicitation": { "form": {} },
    "session": { "notices": {} }
  },
  "clientInfo": { "name": "lightr-vibe", "version": "0.1.0" }
}
```

v2：

```json
{
  "protocolVersion": 2,
  "capabilities": { "elicitation": { "form": {} } },
  "info": { "name": "lightr-vibe", "title": "Lightr Vibe", "version": "0.1.0" }
}
```

- v2 不声明 `fs` / `terminal` / `session.notices` / `session.compaction` / `loadSession`
  （schema 已移除或标记废弃）。
- v2 仍声明 `elicitation.form`，保证 `elicitation/create` 提问路径不回归。
- `_meta.systemPrompt` 仍是 claude 私有扩展；不发送 schema 的顶层 `systemPrompt`
  （未声明 `session.systemPrompt` capability 时发送违反规范）。

### 4.5 initialize 响应解析

```rust
fn initialize_agent_info(protocol: AcpProtocol, result: &Value) -> Value
// v1: result.agentInfo；v2: result.info
fn initialize_version(result: &Value) -> Option<u64>
fn auth_method_names(result: &Value) -> Vec<String>
```

- 保存原始 info 到 `conn.agent_info`，`agent_info_from` 不变。
- `StartResult.protocol_version` 返回实际协商值（幂等复用连接时也读 `conn.protocol`）。
- `authMethods` 非空时记录 `eprintln!` 警告；不阻塞 initialize。
  现有 `classifyAcpError` 已把 `-32000` 映射为认证卡，保持。

### 4.6 probe 协商

`probe_options_inner` 是独立 throwaway 进程，不能复用 `AcpConn`。
抽取纯函数并复用：

```rust
fn probe_initialize(
    writer: &mut BufWriter<ChildStdin>,
    rx: &Receiver<Value>,
    next_id: &mut u64,
) -> Result<(AcpProtocol, Value), String>
```

流程与 4.3 相同，id 每次递增；`probe_read_response` 的 timeout 词汇保持不变。
probe 的 `session/new` 参数与结果归一化复用同一 helper。

## 5. 会话生命周期

### 5.1 方法选择

```rust
fn session_new_params(protocol, cwd, mcp_servers, engine) -> Value
fn session_load_params(protocol, session_id, cwd, mcp_servers, engine) -> Value
fn session_load_method(protocol) -> &'static str // V1: "session/load"; V2: "session/resume"
```

- v1 `session/load`：保持 `{sessionId, cwd, mcpServers, _meta?}`。
- v2 `session/resume`：`{sessionId, cwd, mcpServers, replayFrom:{type:"start"}, _meta?}`。
- v2 `session/new`：`{cwd, mcpServers, _meta?}`（`mcpServers` 可省，但继续显式传注入列表）。
- v2 不支持 `session/load`；`acp_session_load` 的命令名与前端契约不变。

### 5.2 响应归一化

```rust
fn parse_session_result(result: &Value, fallback_id: Option<String>) -> Result<SessionResult, String>
```

- 内部调用 `normalize_config_options` 后再返回。
- v2 `session/resume` 响应没有 `sessionId`，必须使用 `fallback_id`（现有调用已传）。
- `modes` 字段不再使用；前端不消费。
- `ProbeResult.config_options` 同样归一化。

### 5.3 session/close

v2 的 `session/close` 在返回前会推完最终更新和 idle `cancelled`；现有命令名与 30s
默认超时不变。`acp_cancel` 仍发 `session/cancel` notification，并继续把本会话
pending permission 回 `cancelled`（避免 UI 卡住）；turn 是否真正结束以 idle 为准。

## 6. Prompt 生命周期

### 6.1 v1

保持 `rpc_call("session/prompt")` + 活动性超时（两次 update 之间静默 ≥300s），
响应中的 `stopReason` / `usage` 原样映射到 `PromptResult`。

### 6.2 v2

```rust
#[derive(Debug, Clone)]
struct PromptOutcome {
    stop_reason: String,
    usage: Value,
    error: Option<String>,
}
```

`send_prompt(conn, session_id, text) -> Result<PromptResult, String>`：

1. 若 `prompt_waiters` 已有同 session 的 waiter → 返回
   `acp:-32603 该会话已有 prompt 在飞行中`（防止覆盖导致悬挂）。
2. 注册 waiter（channel）。
3. 用 **固定 30s** 发送 `session/prompt`，只等 ack `{messageId}`。
   - ack 错误：移除 waiter，返回错误；`-32800` 追加“消息未被接受（可能已取消）”。
4. ack 成功后 `touch_activity`，循环等待 waiter，1s 轮询：
   - 收到 `PromptOutcome` → 转 `PromptResult`；`error.is_some()` 返回 `Err`。
   - `RecvTimeoutError::Disconnected` → 读取 `dead_reason`，返回 `acp-dead: ...`。
   - 两次活动之间静默 ≥ `PROMPT_IDLE_TIMEOUT_SECS`（300s）→ 返回
     `timeout:300s session/prompt`。
5. 所有退出路径都移除 waiter（用作用域守卫或显式 remove）。

### 6.3 idle 处理

`handle_state_update(conn, session_id, update)`：

- 仅 v2 且 `state == "idle"` 时尝试唤醒 waiter。
- `stopReason` 缺失/非法 → 按 `end_turn` 兼容（与 v1 默认一致，避免委派误判失败）。
- `stopReason == "error"` → `PromptOutcome.error` 取 `update.error`：
  - 对象含 `code/message` → `rpc_error_string`；
  - 字符串 → 原样；
  - 缺失 → `acp:-32603 agent reported error without details`。
- `usage` 若存在则原样放入 `PromptResult.usage`；缺失为 `Null`。
- 唤醒后清空该会话的 `message_text` 累积（turn 已结束）。
- `running` / `requires_action` 只刷新活动时间，不唤醒。
- 没有 waiter 的 idle（resume 重放、后台结束）直接丢弃，不 emit。

### 6.4 取消与断连

- `acp_cancel`：不删除 waiter；等待 agent 的 idle `cancelled`。若 agent 不遵守，
  300s 活动性超时仍会结束。
- `mark_dead` / `disconnect_conn`：清空 `prompt_waiters`（drop sender），
  等待方立即得到 `Disconnected`，不用挂满 300s。
- `run_delegated_child`、`run_automation_turn` 改为调用 `send_prompt`，
  不再直接 `rpc_call("session/prompt")`。

### 6.5 超时辅助

```rust
fn rpc_call_with_timeout(
    conn: &AcpConn,
    method: &str,
    params: Value,
    timeout: Duration,
    activity_based: bool,
) -> Result<Value, String>

fn rpc_call(conn, method, params) // 现有语义：timeout_for + prompt 活动性
```

v2 prompt ack 用 `rpc_call_with_timeout(..., 30s, false)`；`session/resume` 加入
`timeout_for` 的 30s 分支。

## 7. update 归一化

`handle_incoming` 对 `session/update`：

1. `touch_activity`；
2. 提取 `session_id`、`update`、`meta`（`params._meta`）；
3. 若是 v2 `state_update`：先 `handle_state_update`，不 emit；
4. `normalize_update(conn, update)` 返回 `Option<Value>`；
5. 用归一化后的 update 做自动化 summary 累积（仅 `agent_message_chunk`）；
6. emit `acp://update {connId, sessionId, update, meta}`。

### 7.1 映射表

| v2 `sessionUpdate` | 归一化输出 | 说明 |
|---|---|---|
| `agent_message_chunk` | 原样 | `messageId` 已在前端用于消息分组 |
| `agent_thought_chunk` | 原样 | 同上 |
| `user_message_chunk` | 原样（前端忽略） | 本地已有用户消息 |
| `agent_message` | `agent_message_chunk`（只发增量） | 见 7.2 |
| `agent_thought` | `agent_thought_chunk`（只发增量） | 见 7.2 |
| `user_message` | 丢弃 | 不重复渲染 |
| `tool_call_update` | 原样 | 前端首次收到即创建工具卡 |
| `tool_call_content_chunk` | 丢弃（首期） | 完整内容通常随 `tool_call_update.content` 到达 |
| `plan_update` | `{sessionUpdate:"plan", entries}` | 仅 `plan.type=="items"`；整体覆盖 |
| `config_option_update` | 原样 + 键名归一化 | `configId`→`id`，`groupId`→`group` |
| `available_commands_update` | 原样 | 多余 `type` 字段前端忽略 |
| `usage_update` | 原样 | `cost` patch 语义由前端忽略 |
| `session_info_update` | 原样 | 标题/重试逻辑不变 |
| `notice` | 原样 | toast，不入 transcript |
| `state_update` | 不 emit | 只驱动 waiter |
| `terminal_update` / `terminal_output_chunk` | 丢弃 | 本期无 terminal UI |
| 未知/`_` 扩展 | 丢弃 | 不 panic、不阻断后续 |

### 7.2 整消息 upsert 去重

v2 允许 `agent_message` / `agent_thought` 在 chunk 之外发送完整内容（替换语义）。
直接转 chunk 会重复追加，因此用 `conn.message_text: HashMap<String, String>`：

```text
key = "{kind}:{messageId}"，kind = agent | thought
```

- chunk 到达：`entry.push_str(text)`。
- `agent_message` 到达：
  - 展平 `content` 中所有 `type=="text"` 的文本为 `new_text`；
  - 若 `old_text` 是 `new_text` 的前缀 → emit 后缀 chunk；
  - 若没有 `old_text` → emit 全文 chunk；
  - 若内容被改写且不是前缀 → 不 emit（保留已流式内容，避免重复），更新累积值。
- `agent_message` 的 `content` 缺失 → 不 emit、不改累积；
  `content == null` / `[]` → 清空累积，不 emit。
- idle 时清空该会话的 `message_text`，防止长会话无限增长。

### 7.3 配置项归一化

```rust
fn normalize_config_options(value: &mut Value)
```

- 顶层数组每项：`configId` → `id`，`groupId` → `group`。
- `options` 若为分组数组：每个 group 的 `groupId` → `group`；group 内 options 不变。
- 只改这两个键，不碰 `category`、`currentValue`、`type`、`options[].value`。
- 调用点：`session/new` 响应、`session/resume` 响应、`config_option_update`、
  `acp_set_config` 响应、probe 结果。

## 8. 权限归一化

`handle_client_request` 的 `session/request_permission` 改为：

```rust
fn normalize_permission(
    protocol: AcpProtocol,
    params: &Value,
    request_id: &str,
) -> (Value /* toolCall */, Value /* meta */)
```

v1：`params.toolCall` 原样、`params._meta` 原样（现有行为）。

v2：

- `title` 必须进入 `toolCall.title` 兜底；`description` 进入事件 `meta`。
- `subject.type == "tool_call"` → 使用 `subject.toolCall`；首见 `toolCallId` 即创建。
- `subject.type == "command"` →
  ```json
  {
    "toolCallId": "<subject.toolCallId 或 perm-{requestId}>",
    "title": "<params.title>",
    "kind": "execute",
    "status": "pending",
    "rawInput": { "command": "<subject.command>", "cwd": "<subject.cwd>" }
  }
  ```
- `subject` 缺失/未知 → `{toolCallId:"perm-{requestId}", title, kind:"other", status:"pending"}`；
  未知 subject 原始内容放 `rawInput`，便于详情面板展示。
- `meta` 构造：
  - 从 `params._meta` 克隆（对象）或 `{}`；
  - `permission` 对象保留原有键（含 `defaultToNo`、`changes`），补
    `version:1`（缺失时）、`title`、`description`（非空时）；
  - 保留 `claudeCode` 等其它键。
  这样 `parsePermissionTitle` / `parsePermissionDescription` / `isDefaultToNo` 无需改动。
- `options` 原样保留。
- `inject_raw_input_start_line_into` 继续作用于归一化后的 toolCall。

自动化连接仍在 emit 之前直接按 `options[].kind` 自动放行；v2 的 `cancelled` 只表示
权限卡取消，不当作整轮取消，整轮状态由 idle 决定。

## 9. 配置项与模式

```rust
fn set_config_params(protocol, session_id, config_id, value) -> Result<Value, String>
```

- v1：`{sessionId, configId, value}`（现状）。
- v2：额外 `type`：
  - `value.is_boolean()` → `"boolean"`
  - `value.is_string()` → `"id"`
  - 其它 → `Err("acp:-32602 不支持的配置值类型")`
- `acp_set_config` 与自动化/委派的 best-effort 设置共用该 helper。
- 响应先 `normalize_config_options`，再 `detect_config_rejection`（依赖 `id`）。
- v2 的“模式”通过 `config_option_update` / `set_config_option` 传递；
  `current_mode_update` 在 v2 不存在，前端已有的 configOptions 渲染保持不变。

## 10. 错误与认证

- 版本错误：`-32022`；参数/版本错误：`-32602`；prompt 未插入/已取消：`-32800`。
- `initialize_connection` 只在上述版本相关错误上回退 v1；认证 `-32000`、进程退出、
  超时直接返回，避免用 v1 重试掩盖真实问题。
- `-32800` 在 `send_prompt` 中转为可读错误：
  `acp:-32800 prompt 未被接受（可能已取消或关闭）`。
- `authMethods` 非空记录警告；错误卡仍由前端 `classifyAcpError` 的 `-32000` 分支处理。
- 已知限制：本期不做 `auth/login` UI。若 0.104 在无凭据环境要求交互式登录，
  用户需先在终端运行 `claude` 完成一次登录；该限制写入规格。

## 11. 前端最小兼容

核心路径不需要前端改动。仅补两处 v2 枚举值，避免状态卡死：

1. `src/types/acp.ts`：
   - `AcpToolCallStatus` 增加 `'paused' | 'cancelled'`；
   - `AcpPlanEntryStatus` 增加 `'cancelled'`。
2. `src/composables/useConversation.ts` `mapToolStatus`：
   - `cancelled` → `'error'`（停止转圈；语义上不是成功完成）；
   - 其它未知 → `'running'`（保持现状）。
3. `PlanCard.vue`（可选但建议）：`cancelled` 显示“已取消”，中性徽章，避免落到“待办”。

不新增前端依赖，不改 `AcpUpdate` 的 v1 联合类型。

## 12. 回滚与兼容

- 代码层：v1 分支保持原函数与参数；v2 逻辑全部走 `if protocol == V2`。
- 运行时：`LIGHTR_ACP_PROTOCOL=v1` 可强制所有引擎走 v1；用于 0.104 异常时快速回退
  （0.104 会因只支持 v2 握手失败，因此该开关用于保留旧适配器/排查，不用于 0.104）。
- 版本层：`npm i -g @agentclientprotocol/claude-agent-acp@0.102.0` 可回到最后兼容 v1 的版本；
  恢复用 `@0.104.0`。
- 不修改全局安装直到代码验证通过；应用内不自动执行 `npm i -g`。

## 13. 测试策略

### 13.1 Rust 单元测试（纯函数优先）

| 测试 | 断言 |
|---|---|
| `initialize_params_v1_shape` | 有 `clientCapabilities`/`clientInfo`，无 `capabilities`/`info` |
| `initialize_params_v2_shape` | 有 `capabilities`/`info`，无 `clientCapabilities`/`clientInfo` |
| `client_capabilities_v2_only_elicitation` | 只有 `elicitation.form`；无 `fs`/`session`/`terminal` |
| `parse_forced_protocol` | `v1`/`v2`/`auto`/非法值 |
| `should_fallback_to_v1` | `-32022` true；`-32602` true；`-32000`/timeout/dead false |
| `initialize_version_parse` | 2/1/缺失/字符串 |
| `session_load_method_and_params` | v1 `session/load`；v2 `session/resume` + `replayFrom.start` |
| `set_config_params_v2_type` | string→`id`，bool→`boolean`，其它 Err |
| `normalize_config_options` | `configId`/`groupId` 递归映射，保留 `category/currentValue` |
| `normalize_plan_update` | `plan_update` → `plan.entries` |
| `normalize_agent_message_delta` | 首条全量、后续只发后缀、改写不重复 |
| `normalize_permission_tool_call` | tool_call/command/无 subject 三种形状 + meta title/description |
| `prompt_outcome_from_idle` | stopReason/usage/error 映射 |
| `config_rejection_after_normalize` | v2 键名归一化后仍能检测被调整值 |

### 13.2 编译与静态检查

```bash
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
pnpm build
```

### 13.3 真机矩阵

| 场景 | 适配器 | 验证 |
|---|---|---|
| v2 握手 | 0.104.0 | `protocolVersion=2`，agent info 正确 |
| v2 普通回合 | 0.104.0 | 正文/思考/工具卡正常，idle `end_turn` 结算 |
| v2 权限 | 0.104.0 | 标题/目标/描述正确，允许/拒绝生效 |
| v2 计划/配置 | 0.104.0 | `plan_update` 显示；模型/思考强度/模式切换生效 |
| v2 取消 | 0.104.0 | idle `cancelled`，无悬挂 prompt |
| v2 历史恢复 | 0.104.0 | `session/resume` 重放不重复 |
| v2 委派/自动化 | 0.104.0 | `_meta.systemPrompt` + `mcpServers` 生效，summary 正常 |
| v1 回退 | 0.102.0 | 自动回退 v1，普通/权限/取消不回归 |
| v1 引擎 | opencode / codex | 握手保持 v1，行为无变化 |

## 14. 风险与缓解

| 风险 | 缓解 |
|---|---|
| v1-only 适配器拒绝第二次 initialize | ACP v2 文档允许失败后重发；真机 0.102/opencode 验证；若仍失败，再引入“重启进程重试 v1” |
| 0.104 不发 idle / 只发完整 `agent_message` | waiter 300s 活动性超时；整消息增量归一化；真机日志确认 |
| `agent_message` 改写导致重复正文 | 只发前缀增量，非前缀改写不 emit |
| 权限 v2 标题/描述丢失 | 归一化到 `meta.permission.title/description`，复用现有解析器 |
| 配置项键名遗漏 | 所有入口统一 `normalize_config_options`，单测覆盖 |
| 认证卡住 | 识别 `authMethods` 与 `-32000`，给出终端登录指引；不做自动登录 |
| 协议分支散落 | 分支集中在 `AcpProtocol` + helper；组件不感知版本 |

## 15. 文件影响清单

| 文件 | 变更 |
|---|---|
| `src-tauri/src/acp.rs` | 主要实现：协议枚举/协商、连接状态、prompt waiter、update/权限/配置归一化、probe、自动化/委派复用、单测 |
| `src/types/acp.ts` | 工具/计划状态枚举补 `paused`/`cancelled` |
| `src/composables/useConversation.ts` | `mapToolStatus` 支持 `cancelled` |
| `src/views/coding/components/PlanCard.vue` | 可选：`cancelled` 展示 |
| `.trellis/spec/frontend/tauri-bridge.md` | 记录 v2 协商、prompt 生命周期、resume、权限/配置归一化、回退开关 |
| `.trellis/tasks/11-10-acp-v2-claude-agent-upgrade/research/acp-v2-protocol.md` | 补充本次 schema 核对结论（usage 在 idle 为不稳定扩展；`systemPrompt` 顶层字段） |
