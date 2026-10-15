# session/update 全量种类消费

父任务：`10-14-acp-rich-stream-ui`（P3）。前置：`10-13-acp-claude-codex`。研究：
`research/update-kinds.md`（opencode v2 ACP 文档 + codeg `types.rs` 事件表）。

## Goal / 用户价值

前端 `useConversation.applyUpdate` 只消费文本/工具/用量/配置几类 `session/update`；其余种类
（`plan` / `session_info_update` / `available_commands_update` / `current_mode_update`）被当作
`AcpIgnoredEvent` **丢弃**。后果：计划看不到、agent 原生标题不生效、**可重试错误被误当失败**、
`set_config` 被 agent 落成别的值时下拉"无故弹回"、命令/模式面板无数据源。

本期把这些**非纯文本**更新在 UI 全部落地。**不含** AIR 重型（steering/goal/sessionFailure/
contextCompaction 卡/fork/delegation/background）。

## Resolved Decisions（用户已确认）

- **D1 范围=全部**：`plan`（清单或轻卡）、`session_info_update`（标题 + 重试）、
  `ConfigOptionRejected`、`available_commands_update`、`current_mode_update` 都做。
  AIR 重型排除。
- **D2 只消费，不新增能力协商**：这些是 agent 主动发的 notification；不 advertise 新能力
  （如 opencode `session.compaction`），故 `compaction_update` 本期收不到、不做。
- **D3 宽容消费**：收到才渲染，字段缺失/形状未知不报错、不误渲染（版本差异大）。

## Requirements

### R1 研究与字段定稿
- 依据 `research/update-kinds.md` 定字段；实现阶段实机核对 opencode 1.x / codex 真正发的种类。
- 关键形状：`plan` entries `{content, priority?, status?}`；`session_info_update` 的
  `title` 与 `_meta["opencode/retry"]` / `_meta.codex.error`（willRetry）。

### R2 Rust
- `session/update` 的 `update` 已整体透传 → 多数无需改。
- **`ConfigOptionRejected` 需 Rust 侧产生**（非 wire 种类）：`acp_set_config` 调用后，比对
  agent 返回的 configOptions 里该 `configId` 的 `currentValue` 是否 = 请求值；不等 → emit
  `acp://configRejected {connId, configId, requested, actual}`（照 codeg `ConfigOptionRejected`）。
- 若某信息在 `params._meta`（update 外）而不在 update 内，补透传（实测定）。

### R3 前端：逐种类消费
- **`plan`** → `PlanCard`（步骤 + 状态徽章 pending/in_progress/completed）；流式增量按 entries 覆盖。
- **`session_info_update`**：
  - `title` → 更新会话标题（`sessionStore`，优先于本地 `applyAutoTitle`）。
  - 重试：`_meta["opencode/retry"]`（opencode）/ `_meta.codex.error` when `willRetry`（codex）
    → **重试横幅**（非失败卡）；retry=null 或 prompt 响应到达 → 清除。
- **`config_option_rejected`**（Rust emit 的 `acp://configRejected`）→ 轻提示「该选项被 agent 调整」。
- **`available_commands_update`** → 存该会话命令列表（供面板/输入 `/` 菜单）。
- **`current_mode_update`** → 更新当前模式显示（对齐已有 mode config option）。

### R4 类型
- 扩 `src/types/acp.ts`：把要消费的种类从 `AcpIgnoredEvent` **移出**，新增
  `AcpPlanUpdateEvent` / `AcpSessionInfoUpdateEvent` / `AcpAvailableCommandsEvent` /
  `AcpCurrentModeUpdateEvent`；`AcpUpdate` 联合补齐。`sessionStore` 增 `planEntries?` /
  `retryState?` / `availableCommands?`（或等价字段）。

## Acceptance Criteria

- [ ] **AC1** `cargo check` + `cargo test --lib` + `vue-tsc --noEmit` + `pnpm build` 全绿。
- [ ] **AC2** 研究字段落定（`research/update-kinds.md`），实现按其做宽容解析。
- [ ] **AC3** `plan` 不再丢弃：UI 渲染计划清单/卡（含状态）。
- [ ] **AC4** `session_info_update.title` 更新到会话标题。
- [ ] **AC5** 可重试错误显示为**重试横幅**（opencode `_meta.opencode/retry` / codex willRetry），
      非失败卡；恢复后清除。
- [ ] **AC6** `set_config` 被 agent 落成别的值 → 轻提示（Rust 比对 + emit）。
- [ ] **AC7** `available_commands_update` / `current_mode_update` 有数据落点（面板/显示）。
- [ ] **AC8** 未知/未消费种类仍安全忽略（不崩、不误渲染）。
- [ ] **AC9** opencode 零回归：既有文本/工具/权限/提问/用量路径不变。
- [ ] **AC10** 协议路由不含引擎名分支（PAC1）；引擎差异只在数据/消费处。
- [ ] **AC11** 浏览器 mock 不崩（mock 可选补样例）。
- [ ] **AC12** 实机（Tauri，人工）：opencode/codex 会话中确认标题/重试/计划按预期显示。

## Out of Scope

- AIR 重型：steering / goal / sessionFailure / contextCompaction 卡 / fork / 子代理 /
  background tasks / delegation。
- 客户端能力协商（`session.compaction` advertise 等）。
- 每引擎独立 DB 设置。

## Open Questions（实现阶段实机收敛）

- opencode 1.x 实际发哪些？`plan` 是否发？（文档是 v2）
- `session_info_update` 的 `_meta` 键名在 1.x 是否同 v2（`opencode/retry`）。
- `ConfigOptionRejected` 的比对是否可靠（agent 是否总回全量 configOptions）。
