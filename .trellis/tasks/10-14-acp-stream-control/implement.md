# Implement: 流中断 / 停止 / 重连

需求 `prd.md`，设计 `design.md`。顺序：类型 → 空回复推断 → 复核停/重连 → mock → 检查。

## 验证命令

```powershell
cargo check            # workdir: src-tauri
cargo test --lib       # workdir: src-tauri
pnpm vue-tsc --noEmit
pnpm build
```

---

## Step 1 · 前端类型（`src/types/session.ts`）

- [ ] `MessageErrorKind` 增 `'empty'`。
- [ ] 若需 detail 字段（stderr tail），确认 `Message` 可承载（`content` 文本即可）。

**验证**：`pnpm vue-tsc --noEmit`。**回滚点**：`types/session.ts`。

## Step 2 · 空回复失败推断（`useConversation.ts::send` 成功分支）

- [ ] 定义 `turnHasContent(turnId)`：本 turn 是否存在非空文本/思考/工具/error 消息。
- [ ] `result.stopReason === 'end_turn'` 且 `!turnHasContent(turnId)` → 推 `errorKind:'empty'` 的 error 卡；
      并把本 turn 任何 `streaming` 消息 patch 为 `error`（防御）。
- [ ] 不影响 `cancelled`（stopped）/ `refusal`（专门卡）路径。

**验证**：`pnpm vue-tsc --noEmit` + `pnpm build`。**回滚点**：`useConversation.ts`（send 成功分支）。

## Step 3 · 复核取消/停止（`stop` / `closeConnection`）

- [ ] 核对 `stop`：cancel + 权限 drain + 提问 drain；生成中关标签先 cancel 再 disconnect。
- [ ] 核对 stop 后迟到 `end_turn` 不改写已 `stopped` 消息（`finalStatus` 仅 streaming）。
- [ ] 无缺口则仅加注释/断言；有缺口再补。

**验证**：`pnpm vue-tsc --noEmit`。**回滚点**：`useConversation.ts`。

## Step 4 · 复核重试/失败分离

- [ ] 确认重试期间（prompt 在途）不出失败卡；仅 reject/空回复出卡。
- [ ] 无缺口则仅注释。

**验证**：`pnpm vue-tsc --noEmit`。

## Step 5 · 复核/微补重连（`reconnect`）

- [ ] 重连前若 `workspace.isGenerating` → 先 `stop` 收敛本 turn。
- [ ] 核对重连后 live/connecting 状态复位。

**验证**：`pnpm vue-tsc --noEmit` + `pnpm build`。**回滚点**：`useConversation.ts`（reconnect）。

## Step 6 · mock（可选）

- [ ] `acpMock` 加一种"空回复"触发（如输入含「空回复」→ 无 delta、`stopReason:'end_turn'`）供手测。

**验证**：`pnpm build`。

## Step 7 · 全量质量检查（trellis-check）

- [ ] 四门禁全绿。
- [ ] PAC1：无引擎名分支；错误串前缀契约不变。
- [ ] opencode 零回归：正常 turn 不走空回复分支。

## Step 8 · 实机验收（Tauri，人工）

- [ ] 生成中点停止 → 光标收尾为 stopped，无悬挂。
- [ ] 断开/重连 → 连接健康复位。
- [ ] 空回复（mock/构造）→ 失败卡，非静默空。

---

## 关键契约

- **空回复判定保守**：仅 `end_turn` 且**零内容**（工具/文本/思考任一即非空）；cancelled/refusal 不判空。
- **重试 ≠ 失败**：横幅与失败卡互斥。
- **零回归**：正常 turn 与既有错误分类不变。
