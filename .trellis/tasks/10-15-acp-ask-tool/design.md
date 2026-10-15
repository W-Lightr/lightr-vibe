# Design: 注入自有 ask 工具（任意引擎可提问）

需求 `prd.md`。参考 codeg `codeg_mcp.rs` + `acp/delegation/{companion,transport}.rs`（只读）。
复用：`10-14-acp-elicitation` 的 `pending_question` / `acp://question` / `AskQuestionCard` /
`acp_answer_elicitation`。

## 1. 架构（相比 codeg 简化）

```
引擎(任意) ──MCP stdio──▶ lightr-ask-mcp（伴生进程，每会话一个）
                                │  本地 socket（回环 TCP）+ 一次性 token
                                ▼
                       主进程监听器  ──复用──▶ pending_question + emit acp://question
                                ▲
                                │  前端 AskQuestionCard 作答 → acp_answer_elicitation
                                ▼
                       答案 ──socket──▶ 伴生进程 ──MCP tools/call 结果──▶ 引擎/模型
```

与 codeg 的差异：
- codeg 有 `delegation/feedback/sessions/tasks/browser` 多工具 + UDS + 复杂 cancel 竞态；
  我们**只做 `ask_user_question`**，传输用**回环 TCP**（跨平台简单），取消语义最小化。
- 注入方式：codeg 按引擎写 MCP 配置或走 ACP 线；我们**统一走 ACP `session/new.mcpServers`**
  （D2）。

## 2. 伴生进程（新 bin：`src-tauri/src/bin/lightr_ask_mcp.rs`）

- 一个最小 **stdio MCP server**（newline-delimited JSON-RPC over stdin/stdout）。
- 启动参数（照 codeg 三参数裁剪）：`--socket <host:port>` `--token <secret>` `--conn-id <id>`。
- 实现 MCP 方法：
  - `initialize` → 返回 `{ protocolVersion, capabilities: { tools: {} }, serverInfo }`。
  - `tools/list` → `[{ name: "ask_user_question", description, inputSchema }]`。
  - `tools/call`（name=ask_user_question）→ 解析参数 → **经 socket 发请求并阻塞等应答** →
    把答案包成 MCP `content`（文本/结构化）返回；失败/取消 → 返回 declined 语义。
  - `notifications/cancelled` / stdin 关闭 → 取消挂起（关闭 socket future）。
- **inputSchema**（对齐 codeg/Claude Code 形态，本期最小：
  `question`(string, 必填) + `options`(string[], 可选，空=自由文本) + `multiSelect`(bool, 默认 false)；
  `header`/多问留后续）。`questions` 数组形态可在实现时按 codeg 对齐（一次一问）。

> 说明：bin 与主 crate 共享类型困难（bin 是独立 crate target）。选择：
> **小进程只做「透传」**——它不解析业务，只把 `tools/call` 的参数原样经 socket 发给主进程，
> 由主进程 `classify`/渲染；进程内的 schema 是唯一的协议契约（写死在 bin 常量）。

## 3. 主进程：socket 监听 + 桥接（`src-tauri/src/acp_ask.rs` 或并入 `acp.rs`）

- **监听器**：进程内起一个回环 TCP 监听（`127.0.0.1:0` 拿随机端口），或每会话一个监听。
  简化：**全局单监听**，用 `--conn-id` 区分会话（请求带 connId 路由到对应会话）。
- **认证**：每次会话 spawn 生成随机 token，监听器校验（照 codeg）。
- **处理 ask 请求**：`{ connId, token, questions }` → 复用：
  - 生成 request_id，存 `pending_question`（已有），**但 reply 通道是 socket**（不是 JSON-RPC
    id）——故 `pending_question` 的条目需要一个「回主进程 socket 的 sender」而非 rpc_id。
  - emit `acp://question {connId, requestId, sessionId, questions}`（前端弹现有卡）。
  - 用户答 → `acp_answer_elicitation` 命中该 pending → 经 socket 回传答案（而非 send_line）。
- **待定**：`pending_question` 现在存 `(rpc_id, Vec<Question>)` 且回写 JSON-RPC。本任务需要
  **两种应答后端**：ACP elicitation（回 JSON-RPC）+ MCP ask（回 socket）。→ 设计为
  枚举 `QuestionReply { Acp(Value/*rpc id*/), Mcp(oneshot<Answer>) }`，按来源分支回写。

## 4. 注入（`session/new.mcpServers`）

`acp_session_new` / `acp_session_load`（`acp.rs:1355,1377`）与探针（`1035`）当前传 `[]`。
改为构造：
```json
[{ "type": "stdio", "name": "lightr-ask", "command": "<lightr-ask-mcp 路径>",
   "args": ["--socket","127.0.0.1:<port>","--token","<t>","--conn-id","<connId>"],
   "env": [] }]
```
- **bin 路径定位**：与主进程同目录（Tauri sidecar / `current_exe` 同目录）或 dev 下 target 目录。
  实现时定稿（`std::env::current_exe().parent()`）。
- **仅对支持的引擎注入**：opencode 1.x 文档支持 stdio MCP。判定集中在注入处（PAC1：不在协议路由）。
- 探针（throwaway）**不注入**（无会话交互，注入会拉起多余进程）——或注入但立即回收；实现定稿。

## 5. 生命周期 / 取消

- 伴生进程由**引擎**按 MCP 配置拉起；其生命周期与引擎会话绑定（引擎退出即随之退出）。
- 另有 `--parent-pid` 兜底（照 codeg）：主进程/会话死则伴生自退（避免僵尸）。
- 会话关闭/`acp_cancel`：对挂起 MCP ask 回 decline（socket 断开即让伴生返回失败），不 hang。

## 6. 前端

**零改动**（复用 `10-14-acp-elicitation`）：`acp://question` 事件形状不变，`AskQuestionCard`
与 `useAcpQuestion` 照旧。来源差异对前端透明。

## 7. 兼容 / 回滚

- 回滚 = 移除注入（`mcpServers` 恢复 `[]`）+ 删 bin + socket 监听；不影响 ACP elicitation 路径。
- **opencode 零回归**：不调 ask 工具时，仅多一个 MCP server 挂载（无行为变化）。
- 与原生 elicitation **并存**：两来源都进同一卡。

## 8. 测试要点

- 纯函数：MCP 参数 → `Question`（复用 `acp_question::classify` 或新增 MCP→Question 映射）。
- 主进程：socket 请求 → 入 pending → 收到答复 → 回传（可用内存 oneshot 单测桥接层，不真起 socket）。
- 实机（人工）：opencode 1.x 会话，模型调 `ask_user_question` → 弹我们的卡 → 作答 → 继续。

## 9. 风险

- **MCP stdio 字段形状**：opencode 1.x 的 `mcpServers` stdio 项字段（`command`/`args`/`env`/`type`）
  需按 opencode 文档实测确认（文档 §"MCP servers" 示例）。
- **bin 分发**：dev/生产下 `lightr-ask-mcp` 的定位与打包（Tauri sidecar 或同目录）。
- **socket 端口/安全**：回环 + 随机 token；仅本机。
- **模型是否会调用**：注入后模型需"愿意"用该工具（明确 prompt 引导）。
