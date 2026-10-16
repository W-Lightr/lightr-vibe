# Design: 流中断 / 停止 / 重连

需求 `prd.md`，研究 `research`（codeg §5）。桥接规范 `.trellis/spec/frontend/tauri-bridge.md`。
现状代码：`src/composables/useConversation.ts`（`send`/`stop`/`closeConnection`/`reconnect`/
`classifyAcpError`），`src/views/coding/components/MessageComposer.vue`（重试横幅）。

## 1. 现状盘点（已具备）

- `stop(sessionId)`：`cancelQueued`（权限）+ `cancelQueuedQuestions`（提问）+ `acpService.cancel`（session/cancel）。
- `closeConnection`：生成中先 `stop` 再 `acp_disconnect`（关 stdin→kill），摘 live 标记。
- `classifyAcpError`：`acp-dead`→dead / `timeout:`→timeout / `-32000|Authentication required`→auth / 其余 generic。
- 重试横幅：`retryState`（update-kinds 落地）+ turn 边界 `setRetry(null)`。
- `reconnect(sessionId)`：disconnect → 全新 connect + load-or-new。

→ 本期多为**复核 + 补漏**，新增主要是**空回复失败推断**。

## 2. 空回复失败推断（核心新增）

### 2.1 判定（前端，`send()` 成功分支）
turn 内「有内容」的定义：存在属于本 `turnId` 的**任一** assistant 消息且：
- 文本消息 `content.trim() !== ''`，或
- 思考消息有内容，或
- 工具调用存在（`toolCalls?.length`），或
- 已有 error 卡（refusal 等）。
若 `stopReason === 'end_turn'` 且**无上述任一** → 推一条 error 卡：
`{ role:'assistant', type:'error', content:'Agent 未返回任何内容（可能已失败），请重试', errorKind:'empty' }`，
并把本 turn 任何 `streaming` 消息 patch 为 `error`（一般此时无，防御）。
- `stopReason === 'cancelled'`：照旧 stopped，**不判空**。
- `stopReason === 'refusal'`：已有专门卡，**不判空**。

### 2.2 类型
`MessageErrorKind` 增 `'empty'`（`src/types/session.ts`）。空回复卡文案可含 stderr tail（见 2.3）。

### 2.3 Rust 增强（可选，D2 允许降级）
`StderrTail` 已存在（`AcpConn.stderr_tail` 环形缓冲）。`acp_prompt` 可增返回 `stderrTail?: string`
（本 turn 结束时的尾部若干行）。空回复时前端附在 `detail`。**若判定 Rust 改动面/时序收益低，
本期跳过**（前端通用文案已满足 AC2）。

## 3. 取消 / 停止（复核 + 微补）

- `stop`（已有）：核对四件事（cancel、权限 drain、提问 drain、流式收尾）。
- `send()` `finally` 已 `setRetry(null)`、清 `toolCallHosts`、清 `turns`、`setGenerating(false)`。
- **微补（R2）**：`stop()` 本身**不**直接收尾流式消息（靠 prompt response 回 `cancelled` →
  `finalStatus='stopped'`）。若 agent 不响应该 cancel（罕见），turn 会挂到超时 → 由 `catch(kind==='timeout')→stop()`
  再止血。**保持现状**；确认 stop 后迟到 `end_turn` 不把已 stopped 改回 done（`finalStatus` 仅作用于
  `status==='streaming'`，stopped 不在其列 → 满足）。

## 4. 重试 / 失败分离（复核）

- 重试期间 `send()` 仍在 `await acpService.prompt(...)`（未 reject）→ 不会走到 `catch` → **不出失败卡**；
  横幅独立显示。满足"重试期间不出失败卡"。
- turn 边界 `finally` 清 retry。仅 prompt reject 或空回复推断 → 失败卡。**无需改**，加断言式核对。

## 5. 重连复位（复核）

- `reconnect`（已有）：核对 `finally` 正确复位 `reconnecting`/`reconnectingSessions`、
  `liveAcpSessions` 重连后由 connect 重新加入、`refreshAcpStatus` 更新健康。
- **微补（R4）**：重连前若 `workspace.isGenerating(sessionId)`，先 `stop` 收敛本 turn（避免旧 turn 悬挂）。

## 6. 兼容 / 回滚

- 回滚 = 还原 `send()` 空回复分支 + `errorKind`；`stop`/`stop`/`reconnect` 复核若无改动则不涉回滚。
- **opencode 零回归**：正常 turn 不走空回复分支；既有错误分类前缀不变。
- 空回复分支只在「end_turn 且零内容」触发（正常有内容 turn 不触发）。

## 7. 测试要点

- 前端：`vue-tsc` + `build`；mock 可加"空回复"分支（`end_turn` 无 delta）供手测。
- Rust：`cargo test --lib` 保持绿（若加 stderr tail 返回则补测）。
- 实机：停止（流式中）、重连、空回复（mock 注入）各验证一次。

## 8. 风险

- **空回复误判**：把「合法的纯工具回合」当空 → 已用「工具存在即非空」规避；纯思考回合算非空。
- **stderr tail 时效**：Rust 提供的 tail 可能含无关历史 → 只作 `detail`，不参与判定。
- **cancel 无响应**：极少数 agent 不理会 session/cancel → 靠 prompt 超时兜底（既有）。
