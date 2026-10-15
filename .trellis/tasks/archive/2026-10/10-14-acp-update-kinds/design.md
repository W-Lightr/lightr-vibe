# Design: session/update 全量种类消费

需求 `prd.md`，研究 `research/update-kinds.md`。桥接规范 `.trellis/spec/frontend/tauri-bridge.md`。
现状：`useConversation.applyUpdate` switch（`src/composables/useConversation.ts:~362`）。

## 1. 数据流

```
engine ──session/update(notification)──▶ acp.rs reader_loop ──emit──▶ acp://update {connId, sessionId, update}
                                                       │
                                                       └ (set_config 应答) acp_set_config ──比对──▶ acp://configRejected
前端 acpService.listen('acp://update') → dispatchUpdate → useConversation.applyUpdate(switch)
```

- `update` **已整体透传**（含 `plan` / `session_info_update` 的 `update` 内字段）。
- **`session_info_update` 的 `_meta`**：opencode/v2 把 retry 放在 `params.update._meta`（update 内）
  ——已在透传范围内；**若实测在 `params._meta`（update 外）**，Rust 需补透传。design 按 update 内做，
  实机若不符再补（宽容）。
- **`ConfigOptionRejected` 非 wire 种类**：由 Rust 在 `acp_set_config` 后比对产生。

## 2. Rust 改动（最小）

### 2.1 `ConfigOptionRejected`（`acp.rs::acp_set_config`）
现状：`acp_set_config` 已把 agent 返回的 `configOptions` 回给前端（切模型/effort 的落定值）。
扩展：解析返回的 options，找 `configId`（或 `category`）对应项的 `currentValue`：
- == 请求 value → 正常（不变）。
- != 请求 value → 额外 `app.emit("acp://configRejected", {connId, configId, requested, actual})`。
容错：找不到该 configId / 无 currentValue → 不发（不误报）。
> 纯函数 `detect_config_rejection(&Value /*result*/, config_id, requested) -> Option<String /*actual*/>`，可单测。

### 2.2 `session/update` 透传复核
`reader_loop` 对 `session/update`：`update = params.update`（已是全量 clone）。
**确认 `_meta` 是否在 update 内**：ACP `session/update` 的 `update` 对象自带 `_meta` 时自然透传；
若 engine 放 `params._meta`，补 `"meta": params._meta` 到 emit（可选，实测定）。

## 3. 前端改动

### 3.1 类型（`src/types/acp.ts`）
把 `plan` / `session_info_update` / `available_commands_update` / `current_mode_update` 从
`AcpIgnoredEvent` 移出，新增：
```ts
AcpPlanUpdateEvent { sessionUpdate:'plan'; entries?: {content:string; priority?:'high'|'medium'|'low'; status?:'pending'|'in_progress'|'completed'}[] }
AcpSessionInfoUpdateEvent { sessionUpdate:'session_info_update'; title?:string; _meta?:unknown }
AcpAvailableCommandsEvent { sessionUpdate:'available_commands_update'; availableCommands?:{name:string;description?:string}[] }
AcpCurrentModeUpdateEvent { sessionUpdate:'current_mode_update'; currentModeId?:string }
```
`AcpUpdate` 联合补齐；`AcpIgnoredEvent` 仅保留真正不消费的（若有）。

### 3.2 消费点（`useConversation.applyUpdate` 新增 case）
- `plan` → `sessionStore.setPlan(connId, entries)`（`Session` 增 `planEntries?: PlanEntry[]`）；
  渲染 `PlanCard.vue`（消息流内或 composer 上方，按 entries 状态渲染，覆盖式更新）。
- `session_info_update` → 提取 `title`：`sessionStore.renameSession(connId, title)`（仅当未手动改名，
  轻量：直接设，除用户已改名——用 `title` 不以「新会话」/非空判断，简化）；
  重试：解析 `_meta` → `sessionStore.setRetry(connId, {attempt?, message?, nextRetryAt?} | null)`；
  清除时机：`title` 带 retry=null，或 `send()` 的 prompt 响应回来时清（turn 边界）。
- `available_commands_update` → `sessionStore.setCommands(connId, list)`（供输入 `/` 菜单/面板）。
- `current_mode_update` → `sessionStore` 存 `currentModeId`（对齐已有 mode config option 显示）。
- 新事件 `acp://configRejected` → toast「该选项被 agent 调整为 X」+ 乐观值回滚为 actual。

### 3.3 store 字段（`stores/session.ts`，setup store）
`Session` 增可选字段：`planEntries?`、`retryState?`、`availableCommands?`、`currentModeId?`
（均 in-memory，不强制落盘；落盘可选——plan 可随消息一起？本期不落盘，简单）。
增 granular 方法：`setPlan` / `setRetry` / `setCommands` / `setCurrentMode`。

### 3.4 组件
- `PlanCard.vue`（新）：只读清单，每项 `content` + 状态徽章（蓝=进行中/绿=完成/灰=待办）。
- 重试横幅：在 `MessageComposer` 或消息流顶部显示一行「正在重试（第 N 次）…」，`retryState` 驱动。
- `configRejected` 用 naive-ui `message`（既有 `message` 单例）提示。

## 4. 兼容 / 回滚

- 回滚 = 还原 `applyUpdate`/types/store/删除 PlanCard；Rust 的 configRejected 段可留（无人监听即 no-op）。
- **opencode 零回归**：新增分支只对**新种类**生效，既有种类路径逐字节不变。
- 未知种类（未来新增）仍走 default → 忽略。

## 5. 测试要点

- Rust：`detect_config_rejection` 纯函数单测（相等/不等/缺失）。
- 前端：`vue-tsc` + `build`；mock 可补一条 `plan`/`session_info_update` 样例（可选）。
- 实机：opencode/codex 会话确认标题/重试/计划显示。

## 6. 风险

- **opencode 1.x 是否发 plan / retry _meta**：v2 文档为准，1.x 未实测；收到才渲染，收不到无碍。
- **标题冲突**：agent 原生标题 vs 本地 `applyAutoTitle` —— 原生标题优先（收到即覆盖）；
  用户手动改名的保护本期从简（可后续）。
- **ConfigOptionRejected 比对**：依赖 agent 回全量 configOptions；不满足则不发（安全）。
