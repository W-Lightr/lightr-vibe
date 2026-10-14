# Implement: AI 提问 / elicitation 对话框

需求 `prd.md`，设计 `design.md`，事实 `../../10-13-acp-claude-codex/research/codeg-acp-stream-ui.md` §3。
执行顺序：Rust 先（纯函数→请求处理→答复→cancel→能力声明），前端后；每步可独立验证。

## 验证命令（本项目基准）

```powershell
cargo check            # workdir: src-tauri
cargo test --lib       # workdir: src-tauri
pnpm vue-tsc --noEmit
pnpm build
```

---

## Step 0 · 前置实测（可选，阻塞字段名定稿）

若本机已装 `codex-acp`：在真实 codex 会话触发 Plan `request_user_input`，抓一条
`elicitation/create` 的 params JSON，回填 `design.md` §1 的字段名（`mode.requestedSchema.
properties.<id>.enum` vs `enumNames` vs `items.enum`），据此校正宽容解析分支。
未装则按 design §1 的 codeg 形状实现（宽容解析兜底）。

## Step 1 · Rust：纯函数模块 `src-tauri/src/acp_question.rs`

- [ ] `QuestionKind { Single, Multi, Text }`、`QuestionOption { value, label }`、
      `Question { id, prompt, kind, options }`（`Serialize` camelCase）、
      `ElicitationPlan { Questions(Vec<Question>), Decline }`。
- [ ] `classify_elicitation(&Value) -> ElicitationPlan`（宽容解析，见 design §3.1）。
- [ ] `build_elicitation_result(&[Question], &[(String, Vec<String>)], declined) -> Value`。
- [ ] 在 `acp.rs`（或 `lib.rs`）`mod acp_question;` 挂载。
- [ ] 单测：single/multi/text/未知type→Decline/空→Decline；build string/string[]/decline。

**验证**：`cargo test --lib`。
**回滚点**：新文件 + 一行 `mod`，可整删。

## Step 2 · Rust：请求处理接入（`acp.rs::handle_client_request`）

- [ ] `AcpConn` 加 `pending_question: Mutex<HashMap<String,(Value, Vec<Question>)>>` +
      `q_next: AtomicU64`（初始化处同步加）。
- [ ] `handle_client_request`：`elicitation/create` → `classify` → Questions 则存 map +
      `emit("acp://question", {connId, requestId, sessionId, questions})`；Decline 则直回
      `{"action":"decline"}`。**放在现有 `elicitation/` `-32601` 分支之前**（或从该分支
      排除 `elicitation/create`）。
- [ ] 确认 `fs/`、`terminal/` 仍 `-32601` 不变。

**验证**：`cargo check` + `cargo test --lib`。
**回滚点**：`acp.rs`（该分支 + 字段）。

## Step 3 · Rust：答复命令 `acp_answer_elicitation`

- [ ] 新增 `#[tauri::command] acp_answer_elicitation(connId, requestId, answers, declined)`：
      查 `pending_question.remove` → `build_elicitation_result` → `send_line` 回写；未命中 `Ok(())`。
- [ ] `lib.rs` `invoke_handler` 注册。
- [ ] 参数类型 `AnswerItem { field_id: String, values: Vec<String> }`（camelCase serde）。

**验证**：`cargo check`。
**回滚点**：`acp.rs` + `lib.rs`。

## Step 4 · Rust：cancel / 关闭清理（`acp.rs`）

- [ ] `cancel_by_rpc_id`：同时扫 `pending_question`，命中回 `{"action":"decline"}` 并移除。
- [ ] 连接关闭/`acp_disconnect` 清理：遍历 `pending_question` 回 decline。

**验证**：`cargo check` + `cargo test --lib`。
**回滚点**：`acp.rs`。

## Step 5 · Rust：能力声明（仅 codex）

- [ ] `initialize` params 构造：会话引擎 == codex 时加
      `clientCapabilities.elicitation.form = {}`；其他不变。
- [ ] 注释说明「仅 codex」的理由（已知规范 form）+ PAC1（不在协议路由分支）。

**验证**：`cargo check`。
**回滚点**：`acp.rs`。

## Step 6 · 前端：类型 + 服务

- [ ] `src/types/acp.ts`：`AcpQuestion` / `AcpQuestionEvent`。
- [ ] `src/services/acpService.ts`：`answerElicitation(...)` 唯一 invoke + isTauri 双通道 + mock。

**验证**：`pnpm vue-tsc --noEmit`。
**回滚点**：两个文件。

## Step 7 · 前端：状态 + 组件 + 回填

- [ ] 监听 `acp://question` → pending 状态（按 connId）。
- [ ] `AskQuestionCard.vue`（single/multi/text + 提交/跳过）。
- [ ] 挂载到会话视图（composer 上方）。
- [ ] 提交后 push 一条本地 result 消息（问题+答案）。

**验证**：`pnpm vue-tsc --noEmit` + `pnpm build`。
**回滚点**：前端视图/组件/composable。

## Step 8 · 全量质量检查（trellis-check）

- [ ] 四门禁全绿。
- [ ] PAC1：`rg -n "codex|Codex|claude|Claude" src-tauri/src/acp.rs` → 仅能力声明处 + `#[cfg(test)]`；
      协议路由无引擎名。
- [ ] PAC2：`elicitation/create` Questions 应答；Decline 路径不 hang。
- [ ] PAC3：`acp://question` 携带 `connId`；前端按 connId 路由。
- [ ] PAC4：opencode 零回归（不发 elicitation，能力声明不变）。

## Step 9 · 实机验收（Tauri，人工）

- [ ] codex Plan `request_user_input` → 弹提问卡 → 作答 → agent 继续。
- [ ] codex MCP 表单（若有）同上。
- [ ] opencode 会话不受影响。

---

## 关键契约（不可破坏）

- **能力声明仅 codex**（D2）；协议路由不含引擎名（PAC1）。
- **收到 elicitation MUST 回响应**（accept/decline/cancel），否则 agent hang。
- **opencode 零回归**：不发 elicitation 时不产生任何新事件。
