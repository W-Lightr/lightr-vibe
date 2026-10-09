# ACP (Agent Client Protocol) 规范契约

> 调研日期：2026-10-08。来源：官方仓库 `zed-industries/agent-client-protocol`（main 分支）
> `docs/protocol/v1/*.mdx`，对应站点 https://agentclientprotocol.com/protocol/v1/*
> （注意：github.com/agentclientprotocol/agentclientprotocol 目前 404，规范源仓库在 zed-industries 组织下）。
> 规范版本：**v1（protocolVersion: 1，整数）**；仓库已有 v2 草案目录（docs/protocol/v2/），未稳定，本调研按 v1。
> 机器可读 schema 由 `agent-client-protocol-schema`（Rust crate，src/v1/*.rs）生成，站点无独立 schema.json 静态文件。

---

## 1. 传输层（来源：/protocol/v1/transports）

- **stdio 为 SHOULD 支持的基线传输**（Streamable HTTP 仅草案）。
- 帧格式：**newline-delimited JSON-RPC 2.0**（`\n` 分隔），**不是** LSP 的 Content-Length 头。
  - 每条消息是独立的 JSON-RPC request / notification / response。
  - 消息 **MUST NOT 包含内嵌换行**（即一行一条完整 JSON）。
  - 编码 **MUST 为 UTF-8**。
- stdout 只允许出现合法 ACP 消息（agent 不得把日志写 stdout）。
- **stderr 是日志通道**：agent MAY 写任意 UTF-8 日志，client MAY 捕获/转发/忽略。
- 生命周期：client 启动 agent 子进程 → 消息交换 → **client 关闭 stdin 并终止子进程**。

## 2. 方法清单与方向（来源：/protocol/v1/overview + /protocol/v1/schema）

### Client → Agent（requests，除注明外）

| 方法 | 状态 | 说明 |
|---|---|---|
| `initialize` | 基线 | 协商 protocolVersion + capabilities，必须最先调用 |
| `authenticate` | 基线 | params `{ methodId }`；仅在 agent 要求时 |
| `session/new` | 基线 | params `{ cwd, mcpServers, additionalDirectories? }` |
| `session/prompt` | 基线 | params `{ sessionId, prompt: ContentBlock[] }`；**长请求**，响应 `{ stopReason }` |
| `session/load` | 需 `agentCapabilities.loadSession` | params 同 new + sessionId；先用 session/update 重放历史再响应 `{}` |
| `session/resume` | 需 `sessionCapabilities.resume` | 恢复但**不重放**历史 |
| `session/close` | 需 `sessionCapabilities.close` | 取消该会话进行中的工作并释放资源 |
| `session/list` | 需 `sessionCapabilities.list` | `{ cwd?, cursor? }` → `{ sessions: SessionInfo[], nextCursor? }`，游标分页 |
| `session/delete` | 需 `sessionCapabilities.delete` | |
| `session/set_mode` | 可选（**将被废弃**） | `{ sessionId, modeId }` |
| `session/set_config_option` | 可选（新机制） | `{ sessionId, configId, value }` → 返回全量 `configOptions` |
| `logout` | 需 `agentCapabilities.auth.logout` | |
| `session/cancel` | **notification**（无响应） | `{ sessionId }`，取消进行中的 prompt turn |

### Agent → Client

| 方法 | 状态 | 说明 |
|---|---|---|
| `session/update` | **notification** | 流式推送，见 §4 |
| `session/request_permission` | 基线（client 必须能处理） | 见 §5 |
| `fs/read_text_file` | 需 client 声明 `clientCapabilities.fs.readTextFile` | `{ sessionId, path, line?, limit? }` → `{ content: string }`；path 为绝对路径 |
| `fs/write_text_file` | 需 `fs.writeTextFile` | `{ sessionId, path, content }` → `{}` |
| `terminal/create` / `output` / `wait_for_exit` / `kill` / `release` | 需 `clientCapabilities.terminal` | agent 借 client 环境跑 shell |
| `elicitation/create`（+ `elicitation/complete` notification） | 需对应 capability | 结构化向用户收集输入 |

### 协议级

- `$/cancel_request`（notification，双向）：通用请求取消。收到方 MUST 对原请求回一个响应：正常结果或 **error -32800 (Request Cancelled)**。取消是级联的（见 /protocol/v1/cancellation 时序图）。

## 3. 初始化契约（来源：/protocol/v1/initialization）

请求：

```json
{ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
    "protocolVersion": 1,
    "clientCapabilities": { "fs": { "readTextFile": true, "writeTextFile": true }, "terminal": true },
    "clientInfo": { "name": "my-client", "version": "1.0.0" } } }
```

- `clientCapabilities` 全部可选，**省略 = 不支持**（agent MUST 视缺省 capability 为 UNSUPPORTED）。
- 版本协商：client 发自己支持的最新版本；agent 支持则回相同版本，否则回自己支持的最新版本；client 不支持时应断开并提示用户。

响应：

```json
{ "protocolVersion": 1,
  "agentCapabilities": {
    "loadSession": true,
    "promptCapabilities": { "image": true, "audio": true, "embeddedContext": true },
    "mcpCapabilities": { "http": true, "sse": true },
    "sessionCapabilities": { "list": {}, "resume": {}, "close": {}, "delete": {}, "additionalDirectories": {} },
    "auth": { "logout": {} } },
  "agentInfo": { "name": "...", "version": "..." },
  "authMethods": [ { "id": "...", "name": "...", "description": "..." } ] }
```

- **模型列表不在 initialize 里**。会话级模型/mode 通过 `session/new` 响应的 `configOptions`（新机制）或 `modes`（旧机制，将被移除）暴露；切换用 `session/set_config_option`（或旧 `session/set_mode`）。**没有 `session/list_models` 方法**。

## 4. Prompt turn 与流式语义（来源：/protocol/v1/prompt-turn）

流程：client 发 `session/prompt`（**请求保持 pending**）→ agent 期间持续推 `session/update` notifications（可穿插 `session/request_permission` / `fs/*` / `terminal/*` 反向请求）→ turn 结束时 agent 才对 `session/prompt` 回 `{ stopReason }`。

`params.update.sessionUpdate` 判别字段，11 个变体（完整集合）：

| sessionUpdate | 载荷要点 |
|---|---|
| `user_message_chunk` | `{ messageId?, content: ContentBlock }` |
| `agent_message_chunk` | 同上；正文流（渲染为 Markdown） |
| `agent_thought_chunk` | 同上；思考/推理流 |
| `tool_call` | 完整 ToolCall：`{ toolCallId, title, kind?, status?, content?, locations?, rawInput?, rawOutput?, name? }` |
| `tool_call_update` | ToolCallUpdate：除 `toolCallId` 外全部可选，只带变化字段；`content` 是**整体替换**语义 |
| `plan` | `{ entries: [{ content, priority: high|medium|low, status: pending|in_progress|completed }] }` |
| `available_commands_update` | `{ availableCommands: [{ name, description, input? }] }`（slash commands） |
| `current_mode_update` | `{ currentModeId }` |
| `config_option_update` | `{ configOptions: [...] }` |
| `session_info_update` | `{ title?, updatedAt?, _meta? }`（省略的字段不变） |
| `usage_update` | `{ used, size, cost?: { amount, currency } }`（token 用量/费用） |

枚举：
- `ToolKind`: read | edit | delete | move | search | execute | think | fetch | switch_mode | other
- `ToolCallStatus`: pending | in_progress | completed | failed
- `StopReason`: end_turn | max_tokens | max_turn_requests | refusal | **cancelled**（收到 session/cancel 后 MUST 返回 cancelled，即使底层抛异常也要捕获并回这个语义化结果）

`ContentBlock`（与 MCP 兼容的 union，判别字段 `type`）：
- `text`: `{ type:"text", text, annotations? }`（agent MUST 支持）
- `image` / `audio`: `{ type, data(base64), mimeType, uri?, annotations? }`（需对应 promptCapability）
- `resource`（EmbeddedResource）: `{ type:"resource", resource: { uri, mimeType, text|blob } }`（需 `embeddedContext` capability）
- `resource_link`: `{ type:"resource_link", uri, name?, mimeType?, ... }`（agent MUST 支持）

## 5. 取消语义（来源：/protocol/v1/prompt-turn#cancellation + /protocol/v1/cancellation）

- 停止一个 turn：client 发 **`session/cancel` notification**（`{ sessionId }`，无 id、无响应）。
- agent SHOULD：尽快停 LLM 请求、中止进行中的 tool call、发完 pending 的 session/update，然后对原 `session/prompt` 请求回 `stopReason: "cancelled"`。
- **client 义务**：发出 session/cancel 后，对所有 pending 的 `session/request_permission` 反向请求 MUST 回 `{ outcome: { outcome: "cancelled" } }`（不能挂着不回，否则 agent 可能等待）。
- `$/cancel_request` 是通用机制（可取消任意请求，error -32800）；prompt turn 用专用的 session/cancel。

## 6. 权限请求（来源：/protocol/v1/tool-calls + schema#session/request_permission）

Agent → Client 请求：

```json
{ "id": 3, "method": "session/request_permission", "params": {
    "sessionId": "...",
    "toolCall": { "toolCallId": "...", "title": "...", "kind": "execute", "status": "pending", "content": [...] },
    "options": [
      { "optionId": "allow_always_1", "name": "Yes, and auto-accept all actions", "kind": "allow_always" },
      { "optionId": "allow_once_1",   "name": "Yes",                              "kind": "allow_once" },
      { "optionId": "reject_once_1",  "name": "No",                               "kind": "reject_once" } ] } }
```

- `PermissionOptionKind`: allow_once | allow_always | reject_once | reject_always（UI 提示性质用）。
- Client 响应：`{ "outcome": { "outcome": "selected", "optionId": "..." } }` 或 `{ "outcome": { "outcome": "cancelled" } }`。
- `toolCall` 字段类型是 **ToolCallUpdate**（增量形状），不是完整 ToolCall。

## 7. 错误码（来源：schema#ErrorCode）

| code | 含义 |
|---|---|
| -32700 | Parse error（非法 JSON） |
| -32600 | Invalid request |
| -32601 | Method not found |
| -32602 | Invalid params |
| -32603 | Internal error |
| **-32800** | Request cancelled（$/cancel_request 或内部取消） |
| **-32000** | **Authentication required**（session/new 等在未认证时可回此错，client 应引导走 authenticate） |
| -32002 | Resource not found |

## 8. Session Config Options（来源：/protocol/v1/session-config-options）

- **新机制，取代 modes**：agent 在 `session/new` / `session/load` / `session/resume` 响应里 MAY 返回 `configOptions: SessionConfigOption[]`。
- ConfigOption: `{ id, name, description?, category?, type: "select"|"boolean", currentValue, options }`；
  `category` 语义类别含 **`model`**、`mode` 等；select 的 `options` 可为扁平 `{ value, name, description? }[]` 或分组 `{ group, name, options[] }[]`。
- 修改：`session/set_config_option { sessionId, configId, value }` → 响应携带**全量**最新 `configOptions`。
- agent 侧变更通过 `config_option_update` session/update 推送。
- `modes` / `session/set_mode` 为旧机制（SessionModeState: `{ currentModeId, availableModes: [{id,name,description}] }`），规范明言未来移除；两者并存时 client SHOULD 优先用 configOptions。

## 9. 会话持久化相关（来源：/protocol/v1/session-setup、session-list）

- `session/load`：需 `loadSession` capability；agent 先用 `session/update`（user_message_chunk / agent_message_chunk / tool_call...）重放全部历史，再回 `{}`。client 由此重建 UI 时间线。
- `session/resume`：需 `sessionCapabilities.resume`；不重放，直接恢复。
- `session/list`：发现会话（sessionId/cwd/title/updatedAt/_meta），配合 load 实现「历史会话切换」。
- `cwd` MUST 绝对路径；`additionalDirectories`（需 capability）扩展工作区根集合。
- `mcpServers` 为 **required 字段**（可传 `[]`）；stdio 传输 MCP 是所有 agent 必须支持的，http/sse 看 `mcpCapabilities`。

## 10. 对本项目的含义（决策输入）

1. Rust 侧实现一个 **JSON-RPC over 行分隔 stdio** 的 client 即可，无需 LSP 式 Content-Length 帧。
2. 必须处理的四类入站消息：response（按 id 匹配）、`session/update` notification、agent→client **request**（`session/request_permission` 必须实现，`fs/*`、`terminal/*` 按 capability 声明决定是否实现——**不声明就不会收到**）、`$/cancel_request`（可先忽略，收到时回 -32800）。
3. 停止按钮 = 发 `session/cancel` notification + 本地把 pending permission 回 cancelled + 等待 prompt response 的 `stopReason:"cancelled"`。
4. 模型/mode 选择走 `configOptions`（`session/new` 响应携带），不要依赖旧 `modes` 字段。
5. 认证：initialize 响应 `authMethods` 非空且 session/new 回 -32000 时，引导用户终端跑 `opencode auth login`（opencode 的 auth method 是 terminal 型，见实测报告）。
