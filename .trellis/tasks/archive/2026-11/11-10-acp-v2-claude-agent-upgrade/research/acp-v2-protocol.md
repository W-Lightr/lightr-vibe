# ACP v2 与 claude-agent-acp 0.104 研究（2026-11-10）

## 结论

- `@agentclientprotocol/claude-agent-acp` **0.104.0** 为当前 latest，依赖
  `@agentclientprotocol/sdk@2.5.0`、`@anthropic-ai/claude-agent-sdk@0.3.321`，
  `engines.node >= 22`（本机 Node 24.16.0 满足）。
- **0.103.0 起只服务 ACP v2，并移除了 v1 bridge**；0.102.0 是最后一个兼容 v1 的版本。
- 本项目 `src-tauri/src/acp.rs` 当前固定发送 `protocolVersion: 1`，直接升级到
  0.104.0 会收到 `-32022 Unsupported protocol version`，Claude Code 引擎不可用。
- ACP v2 仍是较新的协议线（schema 存在 alpha 版本），0.104.0 与 0.103.0 同日发布，
  必须保留 v1 回退路径，不能全局写死 v2。

## 版本时间线

| 版本 | 日期 | 关键变化 |
|---|---|---|
| 0.90.0 | 2026-10-13 | 本项目当前安装版本，v1 |
| 0.100.0 | 2026-11-02 | 对齐 ACP v2 的 `session/cancel`、`session/close` |
| 0.102.0 | 2026-11-09 | 最后一个带 v1 bridge 的版本 |
| **0.103.0** | 2026-11-10 | **BREAKING：直接服务 ACP v2，移除 v1 bridge**；`lib.ts` 只导出 `serveAcp` 等 |
| **0.104.0** | 2026-11-10 | 新增 ACP auth/status RFD、远端 Anthropic Console 登录 |

来源：
- `https://api.github.com/repos/agentclientprotocol/claude-agent-acp/releases/tags/v0.104.0`
- `https://api.github.com/repos/agentclientprotocol/claude-agent-acp/releases/tags/v0.103.0`
- `https://registry.npmjs.org/@agentclientprotocol%2Fclaude-agent-acp/0.104.0`
- ACP v2 迁移指南：`https://raw.githubusercontent.com/agentclientprotocol/agent-client-protocol/main/docs/protocol/v2/migration.mdx`
- ACP v2 初始化：`https://raw.githubusercontent.com/agentclientprotocol/agent-client-protocol/main/docs/protocol/v2/initialization.mdx`

## v1 → v2 关键差异

### 1. initialize

v1：

```json
{
  "protocolVersion": 1,
  "clientCapabilities": { "fs": {...}, "elicitation": {...}, "session": {...} },
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

- `clientCapabilities` → `capabilities`，`clientInfo` → `info`；`info` 必填。
- 响应：`agentCapabilities` → `capabilities`，`agentInfo` → `info`。
- 版本协商：Agent 选择 ≤ 请求版本的最高支持版本；无交集返回
  `-32022`，`data = { requested, supported }`。
- 客户端能力：
  - `fs`、`terminal` 整组移除；
  - `session.notices`、`session.compaction` 标记移除，v2 notices 可直接下发；
  - `elicitation.form` / `elicitation.url` 对象标记保留；
  - `loadSession` 移除。

### 2. 方法变化

| v1 | v2 |
|---|---|
| `session/load` | **删除**；用 `session/resume` + `replayFrom: { "type": "start" }` |
| `session/resume` | 保留；可选 `replayFrom`，无 capability 门禁 |
| `session/new` | `mcpServers` 可选；响应不再有 `modes` |
| `session/prompt` | 名称/schema 不变，但**响应语义改变** |
| `session/set_mode` | 删除；用 `session/set_config_option` |
| `session/set_config_option` | 参数 `configId` + 必填 `type`（`"id"` 或 `"boolean"`） |
| `session/request_permission` | 参数重构：`title` 必填，`subject` 可选 tagged union |
| `session/cancel` | notification 不变；结束通过 idle `state_update` 确认 |
| `session/close` | 先推完最终更新和 idle `cancelled`，再响应 |
| `authenticate` / `logout` | 改名 `auth/login` / `auth/logout` |

### 3. prompt 生命周期

v1：`session/prompt` 一直挂起到 turn 结束，响应带 `stopReason`。

v2：

- `session/prompt` 只表示用户消息已插入，响应 `{ messageId }`；
- `state_update` 上报前台状态：
  - `running`；
  - `requires_action`（权限/elicitation）；
  - `idle` + `stopReason`（结束）；
- 失败通过 idle `stopReason: "error"` + `error`；
- `usage` 从 prompt 响应移动到 idle `state_update`。

对本项目：`acp_prompt` 不能再用 `rpc_call` 的返回作为 turn 结束；必须等待
该会话的 idle `state_update`，再返回现有 `PromptResult`。

### 4. session/update 变体

| v1 `sessionUpdate` | v2 |
|---|---|
| `user_message_chunk` / `agent_message_chunk` / `agent_thought_chunk` | 保留；`messageId` 必填 |
| `tool_call` | **删除**；首次 `tool_call_update` 创建工具调用 |
| `tool_call_update` | 保留；status/kind 增加可扩展值 |
| `plan` | 改为 `plan_update`，带 `planId` 和 `type`；同 planId 整体替换 |
| `current_mode_update` | 删除；用 `config_option_update` |
| `config_option_update` | 保留；`id` → `configId`，`group` → `groupId` |
| `available_commands_update` | 保留；命令和 `input` 增加必填 `type` |
| `usage_update` | 保留；`cost` 变 patch（省略=不变，null=清空） |
| — | 新增 `state_update`、`user_message`、`agent_message`、`agent_thought`、`terminal_update`、`terminal_output_chunk` |

v2 更新是 upsert：省略字段=不变，`null`=清空，值=替换，chunk 追加；数组字段整体替换。

### 5. 权限请求

v2：

```json
{
  "sessionId": "...",
  "title": "Run this script?",
  "description": "可选说明",
  "subject": {
    "type": "tool_call",
    "toolCall": { "toolCallId": "call_001", "title": "...", "kind": "execute", "status": "pending" }
  },
  "options": [{ "optionId": "allow", "name": "Allow once", "kind": "allow_once" }]
}
```

- `title` 必填；`subject` 可为 `tool_call` / `command`，也可省略；
- `options` 和响应形状不变；
- `cancelled` 不再代表整轮取消，可能只是用户关闭权限卡；整轮取消看 idle `state_update`。

### 6. 本项目私有扩展

核对 0.103.0 源码后确认仍可用：

- `_meta.systemPrompt`：`src/v2/agent.ts` 仍读取 `params._meta.systemPrompt` 并传给
  Claude SDK；本项目的委派系统提示注入可保留。
- `mcpServers`：`src/v2/session-requests.ts` / `agent.ts` 仍把 `session/new` /
  `session/resume` 的 `mcpServers` 映射到 SDK；`lightr-ask-mcp` 注入可保留。
- 但 `session/resume` 每次都要重新传完整 `cwd` / `mcpServers` / `additionalDirectories`。

### 7. 认证

- 0.104.0 新增 `auth/status` 与 `auth/status_update`。
- 如果 `initialize` 返回非空 `authMethods`，客户端必须实现 `auth/login` 与
  `auth/logout`；否则用户可能卡在认证。
- 现有 API key / 本地登录路径需要在真机验证。

## 对本仓库的影响点

| 位置 | 影响 |
|---|---|
| `src-tauri/src/acp.rs` `client_capabilities()` | 需按协议版本返回 v1/v2 形状 |
| `acp.rs` `initialize`（连接 + probe 两处） | 需 v2 参数/响应字段、版本协商与回退 |
| `acp.rs` `session/new`（连接、probe、automation） | v2 `mcpServers` 可选、响应无 `modes`、configOptions 键名 |
| `acp.rs` `session/load` | v2 改 `session/resume` + `replayFrom: start` |
| `acp.rs` `send_prompt` | v2 等待 idle `state_update`，不能等 prompt 响应 |
| `acp.rs` `reader_loop` | 需消费 `state_update`、翻译 v2 update 变体 |
| `acp.rs` permission handler | v2 `title`/`subject` → 现有 `toolCall` 事件 |
| `acp.rs` `acp_set_config` | v2 需 `type: "id"|"boolean"` |
| `acp.rs` automation turn | 复用同一套协议感知 helper |
| `src/types/acp.ts` | 若前端直接消费 v2，需要新类型；本设计选择 Rust 归一化，前端尽量不改 |
| `src/composables/useConversation.ts` | 现有 v1 形状继续消费；新增 v2 变体时只做最小兼容 |

## schema 复核补充（2026-11-10，v2 主分支）

核对 `https://raw.githubusercontent.com/agentclientprotocol/agent-client-protocol/main/schema/v2/schema.json`
与 `docs/protocol/v2/prompt-lifecycle.mdx` 后补充：

- `PromptResponse` 只要求 `messageId`；`usage` 不再出现在 prompt 响应。
- idle `state_update` 的 `usage` 是**不稳定扩展**（schema 未列，migration 文档说明从 v1
  prompt 响应迁移而来）；实现应按可选字段读取，缺失为 `null`。
- idle 的 `stopReason:"error"` 在 `error` 字段携带 JSON-RPC error 对象；`error` 是
  stop reason 的附加字段。
- `PlanUpdate` 形状为 `{plan:{type:"items", planId, entries}}`，不是顶层 `entries`。
- `ConfigOptionUpdate.configOptions[]` 用 `configId`，分组用 `groupId`；`SessionConfigOption`
  的 `type` 为 `"select" | "boolean"`。
- `SetSessionConfigOptionRequest` 的 `type` 是必填判别字段：`"id"` 配字符串值，
  `"boolean"` 配布尔值；未知类型必须 `-32602`。
- `agent_message` / `agent_thought` 是**整消息替换**（`content` 数组整体替换），
  与 chunk 并存；直接转 chunk 会重复正文，需要按 messageId 做增量去重。
- `session/resume` 响应**没有 sessionId**（只有 configOptions/availableCommands），
  调用方必须使用请求里的 sessionId。
- `session/new` 的 `mcpServers` 可省略；`NewSessionResponse` 有 `sessionId`。
- v2 顶层 `systemPrompt` 是规范字段，但只在 Agent 声明 `session.systemPrompt` 后才能发；
  本项目继续使用 claude 的 `_meta.systemPrompt` 私有扩展。
- 初始化文档明确：`initialize` 失败后连接仍未初始化，Client MAY 再次发送 initialize；
  因此 v2 失败/降级后可在**同一连接**重发 v1 握手，不必重启进程。

### `elicitation/create` 形状复核（2026-11-10）

从 npm tarball 直接核对 `@agentclientprotocol/claude-agent-acp@0.104.0`（以及本机
0.90.0）的 `dist/elicitation.js`：

```js
{ mode: "form", sessionId, message, requestedSchema: {...} }
```

即 `mode` 是字符串，`requestedSchema` 在**顶层**（扁平形状）。本仓库
`acp_question.rs` 原先只读 `params.mode.requestedSchema`（嵌套），因此 claude 的
AskUserQuestion / MCP form 会被判为 `Decline`——这是升级前就存在的缺陷。
本次改为先读顶层 `requestedSchema`、再回退嵌套形状，二者都支持；codex/opencode
路径不变。

## 测试矩阵

| 场景 | 适配器版本 | 期望 |
|---|---|---|
| v2 握手 | 0.104.0 | `protocolVersion: 2`，`info` 正确 |
| v2 会话/流式 | 0.104.0 | 正文、思考、工具卡、计划、权限、取消 |
| v2 历史恢复 | 0.104.0 | `session/resume` + replay，无重复 |
| v1 回退 | 0.102.0 / 0.90.0 | 协商回退 v1，现有功能不回归 |
| opencode | 1.19.x | 仍走 v1，无行为变化 |
| codex-acp | 已装版本 | 仍走 v1，无行为变化 |
| 自动化 | 0.104.0 | 无头 turn 正常结束，权限自动放行 |
| 委派 | 0.104.0 | `_meta.systemPrompt` + `mcpServers` 注入仍生效 |
