# 研究：`session/update` 种类消费（opencode / codex）

> 依据：**opencode 提供方 ACP 文档**（`dev.opencode.ai/v2/docs/cli/acp/`，2026-10 抓取）+
> **codeg** 事件表（`src-tauri/src/acp/types.rs:366` `enum AcpEvent`，只读）。
> 本地探针（真实 `opencode acp`）因模型调用不稳定未采到完整流——字段以**文档/参考实现**为准，
> 实现阶段用宽容解析 + 单测锁定；实机再核。

## 本项目现状（基线）

前端 `useConversation.applyUpdate` 的 switch 只处理：
`agent_message_chunk` / `agent_thought_chunk` / `user_message_chunk` / `tool_call` /
`tool_call_update` / `usage_update` / `config_option_update`。
其余（`plan` / `session_info_update` / `available_commands_update` / `current_mode_update`）
是 `AcpIgnoredEvent`（**丢弃**）。

Rust 侧 `session/update` 的 `update` **整体透传**（`acp://update {connId, sessionId, update}`），
故 update 内的字段前端已能拿到；**只是不消费**。若信息在 `params._meta`（update 外），当前**未透传**。

## 1. opencode ACP 文档明确的 `session/update` 种类与形状

opencode 文档 §"Commands / Retry status / Compaction / Compaction marker / Usage / Child sessions"
给出（**v2 文档**，1.x 未必全有——见 §4 风险）：

- **`available_commands_update`**：`{ sessionUpdate:"available_commands_update",
  availableCommands:[{ name, description }] }`。`session/new|load|resume|fork` 后必发，含内置 `compact`。
- **`session_info_update`**（多种 `_meta`，**关键**）：
  - `_meta["opencode/retry"]`：模型调用失败**可重试**时发，`{attempt, nextRetryAt, error:{type,message,status?}}`；
    重试开始时发 `"opencode/retry": null`；**prompt 响应到达时清除重试状态**。
  - `_meta["opencode/compaction"]`：未 advertise `session.compaction` 时用它报压缩进度
    `{status:"started|completed|failed", messageId, reason:"auto|manual", error?}`。
  - **`title`**：原生会话标题（`session_info_update.title`；本机未在 opencode 文档 §标题明列，
    但 ACP 通用 + codeg `NativeSessionTitle` 证实）。
- **`usage_update`**：`{ sessionUpdate:"usage_update", used, size, cost:{amount,currency} }`（已消费）。
- **`compaction_update` / `compaction_summary_chunk`**：仅当 client advertise `session.compaction`
  时才发（本期不 advertise → 收不到，Out of Scope）。

## 2. codeg 的事件表（canonical taxonomy，参考口径）

`types.rs:366` `enum AcpEvent` 里与本任务相关的：
- `PlanUpdate { entries: Vec<PlanEntryInfo> }`（ACP `plan` 更新）→ codeg 前端 `PlanCard`。
- `NativeSessionTitle { title }`（来自 `session_info_update.title`）→ 应用到会话标题（不加 UI）。
- `TurnRetrying { message, error_status?, attempt?, max_retries?, retry_delay_ms? }`：
  codex 的 `session_info_update._meta.codex.error`（仅 `willRetry==true`）+ pi 的 auto_retry →
  **重试横幅**，**不是失败**。
- `ConfigOptionRejected { config_id, option_name, requested, actual, requested_value, actual_value }`：
  `session/set_config_option` 后 agent 落定值 ≠ 请求值 → 提示（否则下拉"无故弹回"）。
  **注意 codeg 是在 Rust 侧比对得出此事件**（set_config 与广播不可区分），非 wire 原生种类。
- `SessionModes` / `ModeChanged { mode_id }` / `SessionConfigOptions`（含 `available_commands`）。
- `Error { message, code?, details? }`（`code` 给前端本地化；`details` 脱敏 stderr tail）。

`plan` 的 entries 形状（ACP 标准 `PlanEntry`）：`{ content, priority?, status? }`
（status: `pending|in_progress|completed`；priority: `high|medium|low`）。

## 3. 各引擎差异

- **opencode 1.x**：核心会发 `usage_update` / `available_commands_update` /（v2）
  `session_info_update._meta["opencode/retry"]`；`plan` 是否发未实测（存疑）。
- **codex-acp**：`session_info_update._meta.codex.error`（willRetry）→ 重试；`plan` 走
  `<proposed_plan>` 文本 + plan-review 权限卡（`codex_plan_review_meta`），不一定发 `plan` 更新。
- **claude-agent-acp**：未细研；按其 ACP 常规应有 `session_info_update`。

## 4. 风险

- **版本差异**：opencode 的 `_meta["opencode/retry"]` 等是 **v2** 文档；1.19.0 未必发 →
  实现须"收到才渲染，收不到不报错"，并以实机确认哪些真到。
- **`plan` 来源不一**：有的引擎走 `plan` 更新，有的走文本/工具卡；统一以 `plan` 更新为准，
  其余不强制。
- **`ConfigOptionRejected` 非 wire 种类**：需在 Rust 侧 set_config 后比对（成本较高）——
  本期**可能降级或后置**，见姊妹研究结论。

## 5. 采纳候选（本任务，按价值/成本）

| 优先级 | 项 | 最小面 | 备注 |
|---|---|---|---|
| **高** | `session_info_update` → 原生标题 | 前端：`title` → `sessionStore` 改标题（对齐 `applyAutoTitle`） | 低风险、直接可见 |
| **高** | 重试横幅 | 前端：`_meta["opencode/retry"]`（opencode）/ `_meta.codex.error` willRetry（codex）→ 横幅；prompt 响应清除 | 避免"可重试"被误当失败 |
| **中** | `plan` → 计划清单/卡 | 前端：`plan` entries → 只读清单（卡可后置） | 引擎支持不一 |
| 低 | `available_commands_update` / `current_mode_update` | 面板数据源 | 本期多半可跳过 |
| 低/后置 | `ConfigOptionRejected` | Rust 比对 set_config 落定值 | 成本高，建议后置 |

**明确 Out of Scope**：AIR 重型（steering/goal/sessionFailure/contextCompaction 卡/fork/
delegation/background tasks）；`session.compaction` advertise（故 `compaction_update` 收不到）。
