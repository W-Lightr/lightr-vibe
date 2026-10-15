# 注入自有 ask 工具（任意引擎可提问）

父任务：`10-14-acp-rich-stream-ui`（P0）。前置：`10-14-acp-elicitation`（已实现 ACP 原生
`elicitation/create` 提问卡链路：`acp://question` 事件 + `AskQuestionCard` + `acp_answer_elicitation`）。
参考：codeg `codeg-mcp`（`src-tauri/src/bin/codeg_mcp.rs` + `acp/delegation/companion.rs`）。

## Goal / 用户价值

`10-14-acp-elicitation` 只能**被动**响应 agent 发的 ACP `elicitation/create`。实测：
- **opencode 1.19.0 不支持** ACP Questions（该功能在 v2 文档里）→ 模型退化成**用 PowerShell
  弹系统 `Show-MessageBox`**（非我们 UI，且卡住 turn）。
- 即便支持，也**无法强制**模型走原生提问；模型可能自造弹窗或直接文字提问。

本期让**任意引擎**都能通过我们的 UI 向用户提问：**自实现一个提问工具并注入引擎**，模型调用
它 → 回连我们 app → 弹**现有**提问卡 → 答案作为工具结果返回。这样：
- 触发可控（模型有一个明确的"提问工具"可用）；
- UI 统一（都走我们的 `AskQuestionCard`，不是系统弹窗/自由文本）。

## Resolved Decisions（用户已确认）

- **D1 新建子任务**：本任务独立规划与实现（父 `10-14-acp-rich-stream-ui`）。
- **D2 注入通道 = MCP stdio**：把自有提问 MCP 服务器写进 `session/new.mcpServers`
  （ACP 线；opencode 1.x 支持 stdio MCP）。与 codeg 的"注入 MCP"思路一致，但我们用 ACP 线
  而非改引擎配置文件。
- **D3 回连方式 = 本地 socket**：MCP 小进程被调用时，通过**本地 socket**（回环 TCP 或
  Unix domain，按平台）回连主进程；主进程弹现有提问卡，答后经 socket 回传结果。
- **D4 复用既有卡链路**：不改提问卡 UI；复用 `acp://question` 事件形状 + `AskQuestionCard`
  + 答案回传语义（Questions 型，单选/多选/文本，一次一问）。
- **D5 参考 codeg 但简化**：不搬 codeg 的 delegation/feedback/sessions/taskboard 等重机制，
  只做 **`ask_user_question` 一个工具**。

## Requirements

### R1 Rust：提问工具「伴生进程」
- 新增一个可执行入口（bin，如 `lightr-ask-mcp`）或子命令：一个 **stdio MCP server**，
  暴露单个工具 `ask_user_question`。
- MCP 工具 schema：`{ questions: [{ id?, question, header?, multiSelect?, options[] }] }`
  （对齐 codeg/Claude Code 形态，本期可只要求 `question` + `options`）。
- 工具调用 → 通过本地 socket 把问题发回主进程 → **阻塞**等答案 → 把答案作为 MCP 工具结果返回。

### R2 Rust：主进程 socket 监听 + 桥接
- 每会话（或全局）起一个本地 socket 监听；MCP 伴生进程启动参数里带**连接信息 + 一次性 token**
  （照 codeg：`--socket-path` + `--token`）。
- 收到 ask 请求 → 复用既有 `pending_question` 机制 → emit `acp://question`（带该会话 connId）
  → 前端弹卡 → 用户答 → 经 socket 回传 → 伴生进程返回工具结果。
- 取消/关闭：会话结束/取消时，对挂起的 ask 回 decline/cancel（伴生进程不得 hang）。

### R3 Rust：把 MCP 服务器注入 `session/new`
- spawn 各会话时，构造 `mcpServers` 项指向我们的伴生进程（stdio：`command` + `args`），
  合并进 `session/new` 的 `mcpServers`（当前传空数组）。
- 仅对**支持 stdio MCP 的引擎**注入（opencode 1.x 支持；其余按引擎能力；不支持的跳过并记日志）。
- 伴生进程随会话生命周期回收（会话关闭即终止）。

### R4 前端：复用提问卡（无需新 UI）
- 提问卡、事件、服务均**复用 `10-14-acp-elicitation` 的现有实现**；本任务不改 UI。
- 验证点：**来源不同**（ACP elicitation vs MCP 工具）但**卡相同**。

### R5 工具结果回传语义
- 用户答 → MCP 工具结果 = 结构化 `{ answers: [...] }`（或文本摘要），使模型能读懂并继续。
- 用户跳过/取消 → 结果标记 declined，模型据此自行判断（不 hang）。

## Acceptance Criteria

- [ ] **AC1** `cargo check` + `cargo test --lib` + `vue-tsc --noEmit` + `pnpm build` 全绿。
- [ ] **AC2** 会话 spawn 时，`session/new.mcpServers` 含我们的提问 MCP 服务器（可注入的引擎）。
- [ ] **AC3** 伴生进程暴露 `ask_user_question`（MCP `tools/list` 可见）；被调用时经 socket
      回连主进程并**阻塞**。
- [ ] **AC4** 主进程收到 ask → emit `acp://question` → 前端弹**现有**提问卡 → 用户答 →
      结果经 socket 回传 → 工具返回；模型继续。
- [ ] **AC5** 跳过/取消/会话关闭：挂起 ask 被 decline/cancel，伴生进程与主进程都不 hang。
- [ ] **AC6** 协议路由不含引擎名分支（PAC1）；"是否注入"的可注入判定集中在 spawn/声明处。
- [ ] **AC7** opencode 会话**不改**其原生路径（文本/工具/权限）行为；仅在确有 ask 工具调用时
      弹卡（PAC4 零回归）。
- [ ] **AC8** 浏览器 mock：无 Tauri 不崩溃（mock 兜底）。
- [ ] **AC9** 实机（Tauri，人工）：opencode 1.x 会话中让模型"用你的提问工具问我一个问题"
      → 弹**我们的卡**（不是系统弹窗/纯文本）→ 作答 → 模型继续。

## Out of Scope

- codeg 的 delegation / feedback / sessions / taskboard / browser 等其余伴生工具。
- 写引擎自有配置文件注入（本任务走 ACP `session/new` 线）。
- ACP 原生 `elicitation/create`（已由 `10-14-acp-elicitation` 完成、并存）。
- 多字段一次多问（沿用一次一问）。
- 认证。

## Open Questions（实现前需实测/定稿）

- **MCP stdio 传参**：`session/new.mcpServers` 的 stdio 项字段形状（`command`/`args`/`env`，
  照 ACP schema）——按 opencode 文档 §"MCP servers" 的 `{ type:"stdio", name, command, args, env }`。
- **socket 形态**：Windows 用回环 TCP（`127.0.0.1:port`）还是命名管道；跨平台优先 TCP。
- **伴生进程启动参数**：`--socket` / `--token` / `--conn-id`（照 codeg 三参数裁剪）。
- **长阻塞与超时**：模型等待 ask 的时长；会话 cancel 时的传播路径。
