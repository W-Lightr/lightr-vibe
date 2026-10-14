# AI 提问 / elicitation 对话框

父任务：`10-14-acp-rich-stream-ui`（P0，最先实施）。前置：`10-13-acp-claude-codex`（引擎接入，
通用 ACP 路径可用）。研究依据：
`../../10-13-acp-claude-codex/research/codeg-acp-stream-ui.md` §3。

## Goal / 用户价值

codex/aclaude 适配器在 Plan 模式或 MCP 工具审批时，会通过 `elicitation/create`
（server→client 请求）向客户端提问或请求确认。**当前项目对 `elicitation/create` 一律回
`-32601`**（`acp.rs::handle_client_request`），导致 codex 的 `request_user_input`、
MCP 表单**静默失败**（agent 侧请求被拒，用户看不到任何提问卡）。

本期让「AI 提问」在对话框可见、可答：把 `elicitation/create` 的 **Questions 型**渲染成
提问卡并回传用户答案；让 codex Plan 提问与通用 MCP 表单走通。

## Resolved Decisions（用户已确认）

- **D1 Questions 优先**：首版只处理 Questions 型（codex Plan `request_user_input`、
  通用 MCP form）。**Approval 型（MCP 工具调用审批 / message-only confirm）本期不做**，
  暂维持现状（后续可复用权限卡）。
- **D2 仅 codex advertise `elicitation.form`**：initialize 能力声明只对 codex 打开
  （已知会发规范 form），其他引擎不声明，避免误触发未知 MCP 表单。
- **D3 题型**：单选（single-select）、多选（multi-select）、自由文本（free-text），
  **一次一问**。boolean/enum/URL/自定义答案等多字段完整形态不在本期。
- **D4 无用户总开关**：本期不做 `ask_user_question` 用户开关（codeg 有；我们后置）；
  收到即渲染。

## Requirements

### R1 Rust：注册 `elicitation/create` 请求处理器
- 在 `handle_client_request` 中，对 `method == "elicitation/create"` 走新处理器
  （不再落入 `elicitation/` 的 `-32601` 分支）。
- 处理器：解析 params → 分类（本期只认 Questions；Approval/未知 → **decline**，
  让 agent 自行判断，不 hang）。
- 与 `request_permission` 同构：生成 `request_id`，存 pending map（键 = JSON-RPC id），
  emit 事件到前端等待答复，答复后回写 JSON-RPC response。

### R2 Rust：Questions 分类（纯函数，可单测）
- `classify_elicitation(params) -> ElicitationPlan`，`ElicitationPlan` 含
  `Questions(Vec<QuestionSpec>)` 与 `Decline`（本期不含 Approval）。
- `QuestionSpec`：`{ id, prompt, kind: Single|Multi|Text, options: Vec<{value,label}> }`。
- 兼容 codex 的 form 形状：从 params 的 `message`/`requestedSchema`（或等价字段）提取
  问题与选项；无法识别 → `Decline`。
- 单测：single-select 映射、multi-select 标记、free-text、未知形状 → Decline。

### R3 Rust：提问答复回传
- `acp_answer_elicitation(connId, requestId, answer)` 命令：按 requestId 找 pending 的
  JSON-RPC id，构造 elicitation 的 `CreateElicitationResponse`（action=accept +
  content/答案），回写 response 并从 map 移除。
- decline/取消路径：`$/cancel_request` 与连接关闭时，对 pending elicitation 回
  cancel 响应（协议要求收到方 MUST 回响应），避免 agent hang。

### R4 Rust：能力声明（D2）
- `initialize` 时，仅当会话引擎是 codex 时声明 `elicitation.form` capability；
  其他引擎不声明（保持现状）。

### R5 前端：提问卡渲染与回答
- 新增 store/状态：`pendingQuestion`（按 connId/requestId 排队；本期一次一张）。
- 事件通道：Rust emit `acp://question` `{connId, requestId, sessionId, questions}`。
- 组件 `AskQuestionCard.vue`：渲染 prompt + 选项（单选/多选）或文本输入；提交调用
  `acp_answer_elicitation`；提交后本地标记已回答。
- 挂载点：对话框内（composer 上方或消息流内，与权限卡一致的展示位置）。
- 服务层：`acpService.answerElicitation()` 唯一 invoke 点，isTauri 双通道 + mock。

### R6 回答回填
- 用户答案以一条本地消息/卡片回填进会话（对齐 codeg `ask-question-result-card`），
  使用户能看到「问了什么、答了什么」。

## Acceptance Criteria

- [ ] **AC1** `cargo check` + `cargo test --lib` + `vue-tsc --noEmit` + `pnpm build` 全绿。
- [ ] **AC2** `elicitation/create` **不再** 落入 `-32601`；Questions 型能 emit
      `acp://question` 并等待答复。
- [ ] **AC3** `classify_elicitation` 纯函数单测覆盖 single/multi/text/未知→Decline。
- [ ] **AC4** `acp_answer_elicitation` 正确回写 JSON-RPC response（含 action、答案），
      pending 条目被清除；重复/过期 requestId 安全 no-op。
- [ ] **AC5** 连接关闭或 `$/cancel_request` 时，pending elicitation 得到 cancel 响应
      （agent 不 hang）。
- [ ] **AC6** 提问卡渲染 prompt + 单选/多选/文本；提交后 UI 标记已回答、按钮禁用。
- [ ] **AC7** 答案回填为一条可见卡片，含问题与答案。
- [ ] **AC8** 仅 codex advertise `elicitation.form`；其他引擎 initialize 能力不变。
- [ ] **AC9** 协议路由不含引擎名分支（PAC1）：codex 判定只在能力声明处，处理器本身通用。
- [ ] **AC10** opencode 会话零回归（PAC4）：既有文本/工具/权限路径不变。
- [ ] **AC11** 浏览器 mock：无 Tauri 时不崩溃（事件/服务层 mock 兜底）。
- [ ] **AC12** 实机（Tauri，人工）：codex Plan `request_user_input` 弹提问卡并可答、
      agent 继续（依赖已装 `codex-acp`）。

## Out of Scope

- Approval 型 elicitation（MCP 工具审批 / message-only confirm）→ 后续复用权限卡。
- boolean / enum / URL elicitation / 自定义答案 / `none_of_the_above`。
- 多字段一次多问（本期一次一问）。
- `ask_user_question` 用户总开关。
- 认证。
