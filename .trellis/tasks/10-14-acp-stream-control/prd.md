# 流中断 / 停止 / 重连

父任务：`10-14-acp-rich-stream-ui`（P2，最后一个子任务）。前置：`10-13-acp-claude-codex` +
`10-14-acp-update-kinds`（重试横幅已落地）。研究：`../../10-13-acp-claude-codex/research/codeg-acp-stream-ui.md` §5。

## Goal / 用户价值

对话流的**异常与中断**路径要可靠、可理解，不能留下「永久打字光标」「看起来卡死」「失败与重试混为一谈」等
体验缺陷。当前多数原语已存在（`stop`→`session/cancel`、`closeConnection` 杀进程、错误分类
dead/timeout/auth/generic、重试横幅、`reconnect`），本期**补齐缺口并打磨**：

1. **空回复静默失败**：agent 报成功（`end_turn`）但本 turn **无任何内容**（无文本/思考/工具）
   → 当前会收尾成空的 done，用户看不到任何反馈。需**推断为失败**并给可读提示（照 codeg
   `turn_failed_empty*`，用 stderr tail 兜底）。
2. **取消语义完整性**：生成中 `stop`/关标签时，进行中的权限卡/提问卡必须一并收敛、进程按约定终止、
   迟到 update 不产生悬挂流式消息。
3. **重试 ≠ 失败**：可重试错误只出**重试横幅**，不出失败卡；两者不叠加。
4. **重连一致性**：`reconnect` 后连接健康、live 标记、流式状态正确复位；不残留旧进程/旧请求。

## Resolved Decisions（用户已确认）

- **D1 只做流控制，不引入新协议能力**：复用 ACP `session/cancel`、`$/cancel_request`、既有错误串前缀；
  不新增能力协商。
- **D2 空回复推断为失败**：`end_turn` 且 turn 内零内容 → `errorKind: 'empty'`（新）+ stderr tail 摘要
  （若 Rust 提供）；非 `end_turn` 的正常收尾不受影响。
- **D3 重试与失败分离**：`TurnRetrying`（横幅）与失败卡互斥；重试期间不出失败卡。
- **D4 范围内**：停止/取消、空回复推断、重试/失败分离、重连复位。**不含**：自动重连策略（backoff/
  无限重试）、断网检测、进程池。

## Requirements

### R1 空回复失败推断（前端 + 可选 Rust）
- `send()` 收尾时：若 `stopReason !== 'cancelled'` 且本 `turnId` 下**无任何** assistant 文本/思考/工具/错误
  消息 → 视为**空回复失败**：patch 本 turn 的流式消息为 `error`（或推一条 `error` 卡），文案
  「Agent 未返回任何内容（可能已失败），请重试」。
- **Rust（可选增强）**：`acp_prompt` 响应或连接可提供**本 turn stderr tail**（既有 `stderr_tail` 环形
  缓冲）→ 空回复时回给前端作明细。若成本高则前端先用通用文案（D2 允许降级）。
- 单测/手测：空 turn → 出 error 卡；正常 turn → 不受影响。

### R2 取消/停止语义（复核 + 补漏）
- 生成中 `stop(sessionId)`：`session/cancel` + `cancelQueued`（权限）+ `cancelQueuedQuestions`（提问）
  + 本 turn 流式消息收尾为 `stopped`（不悬挂光标）。
- `closeConnection(sessionId)`：生成中先 cancel 再 `acp_disconnect`（核对该路径已覆盖）。
- 迟到 `session/update`（cancel 后到达）：因 `turnId` 已清，不再新建悬挂流式消息（复核）。
- **补漏点**：stop 后若 prompt response 迟到回 `end_turn`，不应把已 stopped 的消息改回 done
  （复核 `finalStatus` 只对 `streaming` 生效即可）。

### R3 重试/失败分离（复核 + 补漏）
- 重试横幅由 `retryState` 驱动（已在 update-kinds 落地）；**重试期间** prompt 仍在途，
  不得同时渲染失败卡。
- 仅当 prompt 真正 reject（或空回复推断）才出失败卡。
- 复核：turn 边界 `setRetry(null)` 与失败卡生成不冲突。

### R4 重连复位（复核 + 补漏）
- `reconnect(sessionId)`：`acp_disconnect` → 全新 connect + load-or-new；复位本会话的
  live/connecting 状态；本 turn（若有）收敛。
- 复核：重连后健康芯片、`liveAcpSessions`、`acpConnectingSessionIds` 正确；不残留旧 pending。

## Acceptance Criteria

- [ ] **AC1** `cargo check` + `cargo test --lib` + `vue-tsc --noEmit` + `pnpm build` 全绿。
- [ ] **AC2** 空回复（`end_turn` 且本 turn 零内容）→ 出**失败卡**（`errorKind: 'empty'`），非静默空 done。
- [ ] **AC3** 生成中点停止 → 本 turn 流式消息收尾为 `stopped`（无悬挂光标）；进行中权限/提问卡收敛。
- [ ] **AC4** 关标签/删会话（生成中）→ cancel + 杀进程，无悬挂、无僵尸。
- [ ] **AC5** 可重试错误只出**重试横幅**，不出失败卡；重试结束（成功/失败/取消）横幅清除。
- [ ] **AC6** `reconnect` 后：连接健康/live 标记复位；可继续对话；无残留旧进程。
- [ ] **AC7** opencode 零回归：正常 turn、既有错误分类（dead/timeout/auth/generic）行为不变。
- [ ] **AC8** 协议路由不含引擎名分支（PAC1）；错误串前缀契约不变。
- [ ] **AC9** 浏览器 mock 不崩（mock 可补一条空回复样例，可选）。
- [ ] **AC10** 实机（Tauri，人工）：opencode/codex 会话中各验证停止、重连、空回复（可 mock 触发）。

## Out of Scope

- 自动重连 / backoff / 无限重试策略。
- 断网/网络抖动检测（非 ACP 层）。
- AIR 重型（steering/goal/sessionFailure/contextCompaction/fork/delegation/background）。
- 进程池 / 多进程复用。
- 认证流程改造。

## Open Questions（实现阶段定稿）

- 空回复推断是否需要 Rust 提供 stderr tail（成本 vs 价值）？先做前端通用文案，Rust 增强视情况。
- `end_turn` 但只有思考块（无最终文本）算不算空？倾向：**算有内容**（有思考即非空），仅「零消息」才判空。
- `refusal` stopReason 与「空回复」的关系（refusal 已有专门卡，二者不叠加）。
