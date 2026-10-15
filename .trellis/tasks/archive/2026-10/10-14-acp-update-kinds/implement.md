# Implement: session/update 全量种类消费

需求 `prd.md`，设计 `design.md`，研究 `research/update-kinds.md`。
顺序：Rust（configRejected 纯函数 + emit）→ 前端（类型 → store → applyUpdate → 组件）→ 检查。
每步可独立验证。

## 验证命令

```powershell
cargo check            # workdir: src-tauri
cargo test --lib       # workdir: src-tauri
pnpm vue-tsc --noEmit
pnpm build
```

---

## Step 0 · 实机/字段核对（可穿插）

- [ ] 实机确认 opencode 1.x 是否发 `plan` / `session_info_update.title` /
      `_meta["opencode/retry"]`；codex 的 `_meta.codex.error`。回填 research。
- [ ] 确认 `session_info_update` 的 `_meta` 在 `update` 内还是 `params._meta`（决定是否补 Rust 透传）。

## Step 1 · Rust：`detect_config_rejection` + emit

- [ ] 纯函数 `detect_config_rejection(result: &Value, config_id: &str, requested: &str) -> Option<String>`：
      在 `result.configOptions` 找 `id|category == config_id` 的项，`currentValue` 为字符串且 != requested → Some(actual)。
- [ ] `acp_set_config`：调用后比对，不等则 `app.emit("acp://configRejected", {connId, configId, requested, actual})`。
- [ ] 单测：相等→None；不等→Some；缺项/无 currentValue→None。

**验证**：`cargo test --lib`。**回滚点**：`acp.rs`（该函数 + 调用点）。

## Step 2 · Rust：`session/update` 透传复核

- [ ] 若 Step 0 发现 `_meta` 在 `params._meta`（update 外），在 `reader_loop` 的 `session/update`
      emit 里补 `"meta": params._meta`（可选）；否则不改。

**验证**：`cargo check`。**回滚点**：`acp.rs`。

## Step 3 · 前端：类型（`src/types/acp.ts`）

- [ ] 从 `AcpIgnoredEvent` 移出 `plan` / `session_info_update` / `available_commands_update` /
      `current_mode_update`；新增对应 interface（见 design §3.1）。
- [ ] `AcpUpdate` 联合补齐；`AcpConfigRejectedEvent`（若走类型）。

**验证**：`pnpm vue-tsc --noEmit`。**回滚点**：`types/acp.ts`。

## Step 4 · 前端：store 字段与方法（`stores/session.ts`）

- [ ] `Session` 增 `planEntries?` / `retryState?` / `availableCommands?` / `currentModeId?`。
- [ ] 方法 `setPlan` / `setRetry` / `setCommands` / `setCurrentMode`。

**验证**：`pnpm vue-tsc --noEmit`。**回滚点**：`session.ts`。

## Step 5 · 前端：applyUpdate 新增分支（`useConversation.ts`）

- [ ] `plan` → `setPlan`。
- [ ] `session_info_update` → 标题 `renameSession`；`_meta` 重试 → `setRetry`；
      turn 边界（prompt 响应到达）清 retry。
- [ ] `available_commands_update` → `setCommands`。
- [ ] `current_mode_update` → `setCurrentMode`。
- [ ] `acpService` 增 `acp://configRejected` 监听 → dispatch → toast + 回滚乐观值。

**验证**：`pnpm vue-tsc --noEmit` + `pnpm build`。**回滚点**：`useConversation.ts` + `acpService.ts`/`acpEvents.ts`。

## Step 6 · 前端：组件

- [ ] `PlanCard.vue`（新）：只读清单 + 状态徽章；挂到消息流/会话视图。
- [ ] 重试横幅（`MessageComposer` 或消息流顶）由 `retryState` 驱动。
- [ ] `configRejected` toast（复用 naive-ui `message`）。

**验证**：`pnpm vue-tsc --noEmit` + `pnpm build`。**回滚点**：视图/组件。

## Step 7 · 全量质量检查（trellis-check）

- [ ] 四门禁全绿。
- [ ] PAC1：协议路由无引擎名分支。
- [ ] opencode 零回归（既有种类路径不变）。
- [ ] 未知种类仍安全忽略。

## Step 8 · 实机验收（Tauri，人工）

- [ ] opencode：原生标题显示、重试横幅（若触发）、plan（若发）。
- [ ] codex（若装）：willRetry → 重试横幅。
- [ ] set_config 被落定别的值 → 提示。

---

## 关键契约

- **只消费，不加能力协商**；AIR 重型排除。
- **宽容**：字段缺失/形状未知不报错、不误渲染。
- **零回归**：既有种类路径逐字节不变。
