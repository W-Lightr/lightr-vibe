# Implement — ACP v1/v2 双栈与 claude-agent-acp 0.104 适配

> 执行顺序按依赖排列。每步完成即可编译；不要一次性重写 `acp.rs`。
> 所有改动只落在 design §15 列表内；不执行 `npm i -g`。

## 0. 准备与基线

1. 确认工作区只有本任务目录未跟踪：
   ```bash
   git status --short
   ```
2. 记录基线：
   ```bash
   cargo test --manifest-path src-tauri/Cargo.toml
   pnpm build
   ```
3. 阅读：
   - `research/acp-v2-protocol.md`
   - `design.md` §4–§10
   - `.trellis/spec/frontend/tauri-bridge.md` 的 ACP 事件/超时/权限章节

**完成标志**：基线命令通过，明确当前 `acp.rs` 3102 行结构与 `AcpConn` 字段。

## 1. 协议模型与纯函数（先不加分支调用）

在 `src-tauri/src/acp.rs` 顶部工具区新增：

- `enum AcpProtocol { V1, V2 }` + `as_u8/from_u8/from_version`。
- `fn parse_forced_protocol(raw: Option<&str>) -> Option<AcpProtocol>`。
- `fn forced_protocol() -> Option<AcpProtocol>`（读 `LIGHTR_ACP_PROTOCOL`）。
- `fn client_capabilities_for(protocol) -> Value`（保留旧 `client_capabilities()` 作为 v1
  包装，避免一次性改所有测试）。
- `fn initialize_params(protocol) -> Value`。
- `fn initialize_version(result: &Value) -> Option<u64>`。
- `fn initialize_agent_info(protocol, result) -> Value`。
- `fn auth_method_names(result) -> Vec<String>`。
- `fn should_fallback_to_v1(err: &str) -> bool`：
  - `acp:-32022` → true
  - `acp:-32602` → true（仅 initialize 使用）
  - 其它 → false
- `fn combine_initialize_errors(v2: &str, v1: &str) -> String`。

**验证**：`cargo check`；为上述纯函数补单测（design §13.1 前 6 条）。

## 2. 连接协议状态

1. `AcpConn` 增加：
   ```rust
   protocol: AtomicU8,
   prompt_waiters: Mutex<HashMap<String, Sender<PromptOutcome>>>,
   message_text: Mutex<HashMap<String, String>>,
   ```
2. `connect_inner` 初始化 `protocol: AtomicU8::new(0)`、两个空 map。
3. 增加 `impl AcpConn { fn protocol(&self) -> AcpProtocol }`。
4. `connect_inner` 幂等分支返回 `conn.protocol().as_u64()`，不再写死 1。

**验证**：`cargo check`。

## 3. initialize 协商（有状态连接）

1. 新增：
   ```rust
   fn initialize_connection(conn: &AcpConn) -> Result<AcpProtocol, String>
   ```
2. 逻辑严格按 design §4.3：
   - 允许 v2 时先发 v2（`rpc_call(conn, "initialize", initialize_params(V2))`）。
   - 响应版本 2 → 存 `protocol=2`、`agent_info=info`、记录 auth 警告，返回 V2。
   - 响应版本 1 → 同一连接再发 v1，确认版本 1 后存 `protocol=1`、`agent_info=agentInfo`。
   - v2 返回 `-32022`/`-32602` → 同一连接发 v1；成功返回 V1。
   - 两次失败 → `combine_initialize_errors`。
   - `forced_protocol()==Some(V2)` 时只发 v2；`Some(V1)` 时只发 v1。
3. `connect_inner` 用 `initialize_connection(&conn)?` 替换现有固定 v1 握手。
   - 失败路径保留现有 kill/remove。
   - 返回 `StartResult.protocol_version = protocol.as_u8() as u64`。
4. 删除/改造旧的 `initialize_params` 内联 JSON。
5. 更新 `client_capabilities_advertises_session_notices_only` 等旧测试，新增 v2 能力测试。

**验证**：
```bash
cargo test --manifest-path src-tauri/Cargo.toml client_capabilities
cargo test --manifest-path src-tauri/Cargo.toml initialize
```

## 4. probe 协商

1. 新增 `probe_initialize(writer, rx, next_id) -> Result<(AcpProtocol, Value), String>`。
2. 重写 `probe_options_inner` 的 initialize 段：
   - 用 `probe_initialize` 得到协议与结果；
   - `session/new` 参数按协议构造（v2 可省略 `mcpServers`，仍传 `[]`）；
   - 结果经 `normalize_config_options` 后取 `configOptions`。
3. 保持 probe 的超时词汇与 `ProbeChild` 收尸逻辑不变。

**验证**：单测 `probe` 相关纯函数；真机 `acp_probe_options`（opencode 与 claude 0.104）
不挂起、能取到模型列表。

## 5. 会话生命周期

1. 新增：
   ```rust
   fn session_load_method(protocol) -> &'static str
   fn session_new_params(protocol, cwd, mcp_servers, engine) -> Value
   fn session_load_params(protocol, session_id, cwd, mcp_servers, engine) -> Value
   ```
   - v2 load 用 `session/resume` + `replayFrom: {"type":"start"}`。
   - 所有参数继续经 `with_session_meta`。
2. `acp_session_new`、`acp_session_load`、`run_delegated_child`、`run_automation_turn`
   改用 helper；`session/load` 的方法名按协议选择。
3. `parse_session_result` 调用 `normalize_config_options`。
4. `timeout_for` 增加 `"session/resume" => 30s`。

**验证**：单测 `session_load_method_and_params`、`normalize_config_options`；
`cargo check`。

## 6. Prompt 生命周期

1. 定义 `PromptOutcome { stop_reason, usage, error }`。
2. 重构 `rpc_call`：
   - 保留 `rpc_call` 签名与现有语义；
   - 抽出 `rpc_call_with_timeout(conn, method, params, timeout, activity_based)`；
   - 错误串仍为 `timeout:{secs}s {method}`。
3. 新增：
   ```rust
   fn handle_state_update(conn: &AcpConn, session_id: &str, update: &Value)
   fn send_prompt(conn: &AcpConn, session_id: &str, text: &str) -> Result<PromptResult, String>
   ```
4. `send_prompt` 按 design §6.2 实现 waiter 注册/移除/ack/等待/超时。
5. `acp_prompt` 改为调用 `send_prompt`。
6. `run_delegated_child`、`run_automation_turn` 的 prompt 段改为 `send_prompt`。
7. `mark_dead`、`disconnect_conn` 清空 `prompt_waiters`。
8. `handle_incoming` 的 `session/update` 分支：
   - v2 `state_update` 先调 `handle_state_update`，不 emit；
   - 再归一化并 emit。

**验证**：单测 `prompt_outcome_from_idle`；`cargo test`；
真机 0.104 普通回合能等到 idle 后收尾（不提前结束）。

## 7. update 归一化

1. 新增：
   ```rust
   fn normalize_update(conn: &AcpConn, update: Value) -> Option<Value>
   fn normalize_config_options(value: &mut Value)
   fn normalize_agent_upsert(conn: &AcpConn, kind: &str, update: &Value) -> Option<Value>
   ```
2. 按 design §7 映射表实现；未知变体返回 `None`。
3. `handle_incoming` 的自动化 summary 累积改为读取归一化后的
   `agent_message_chunk`（否则 v2 整消息不会进 summary）。
4. `handle_state_update` 在 idle 时清空 `message_text`。
5. `normalize_config_options` 同时用于 `config_option_update`。

**验证**：单测 `normalize_plan_update`、`normalize_agent_message_delta`；
`cargo test`。

## 8. 权限归一化

1. 新增：
   ```rust
   fn normalize_permission(protocol, params, request_id) -> (Value, Value)
   ```
   按 design §8 实现 tool_call/command/无 subject 与 meta 合并。
2. `handle_client_request` 的 `session/request_permission`：
   - v1 行为不变；
   - v2 调 `normalize_permission`；
   - 其余自动放行/emit/pending 逻辑不变。
3. `inject_raw_input_start_line_into` 继续作用在归一化后的 toolCall。

**验证**：单测 `normalize_permission_tool_call` 三种 subject；`cargo test`。

## 9. 配置项

1. 新增 `set_config_params(protocol, session_id, config_id, value) -> Result<Value, String>`。
2. `acp_set_config` 改用该 helper；响应先归一化再 `detect_config_rejection`。
3. `run_automation_turn` 的模型/思考强度设置改用该 helper（best-effort 不变）。
4. v2 的 `type` 规则：字符串 `"id"`、布尔 `"boolean"`、其它返回 `-32602`。

**验证**：单测 `set_config_params_v2_type`、`config_rejection_after_normalize`。

## 10. 前端最小兼容

1. `src/types/acp.ts`：
   - `AcpToolCallStatus` 加 `'paused' | 'cancelled'`；
   - `AcpPlanEntryStatus` 加 `'cancelled'`。
2. `src/composables/useConversation.ts` `mapToolStatus`：
   - `case 'cancelled': return 'error'`。
3. `src/views/coding/components/PlanCard.vue`：
   - `cancelled` → 标签“已取消”、中性徽章；图标保持 `Circle` 或改用 `X`（不新增依赖）。

**验证**：`pnpm build`；无新增 TS 错误。

## 11. 全量验证

```bash
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
pnpm build
```

真机（代码验证通过后才安装 0.104.0）：

1. `npm i -g @agentclientprotocol/claude-agent-acp@0.104.0`
2. Claude Code 新建会话 → 握手 `protocolVersion=2`。
3. 普通回合：正文/思考/工具卡；结束不提前。
4. 权限卡：标题/目标/描述；允许/拒绝。
5. 计划、模型/思考强度/模式切换。
6. 停止生成 → idle `cancelled`。
7. 历史恢复 → `session/resume` 无重复。
8. 委派子任务 + 自动化运行。
9. 回退验证：`npm i -g ...@0.102.0`，确认自动回退 v1；再恢复 0.104.0。

## 12. 回滚点

| 阶段 | 回滚方式 |
|---|---|
| 步骤 1–3 后握手异常 | `git checkout -- src-tauri/src/acp.rs` 回到固定 v1；或用 `LIGHTR_ACP_PROTOCOL=v1` 验证旧适配器 |
| 步骤 6 后 prompt 异常 | 临时用 `LIGHTR_ACP_PROTOCOL=v2` 排除 v1 干扰；必要时回退步骤 6 的 commit |
| 真机 0.104 异常 | `npm i -g @agentclientprotocol/claude-agent-acp@0.102.0`；代码保留双栈 |
| 规格文档 | 最后一步提交前可单独 review，不影响运行时代码 |

## 13. 禁止事项

- 不执行 `npm i -g` 直到 `cargo test` + `pnpm build` 通过。
- 不按引擎名分支协议；不把 `protocolVersion` 写死为 2。
- 不改 `AcpConn` 的 per-session 进程模型、事件名、`connId` 路由。
- 不在前端新增 ACP v2 类型分支；归一化只在 Rust。
- 不删除 v1 路径、不改 opencode/codex 的启动/探测。
- 不把 `agent_message` 直接当 chunk 追加（会重复正文）。
- 不让 v2 prompt 等待 `session/prompt` 响应作为 turn 结束。
- 不把 API key / 认证信息写日志或写入任务文档。
