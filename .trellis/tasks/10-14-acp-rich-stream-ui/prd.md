# ACP 富交互与流处理（父任务）

父任务：无（顶层）。前置：`10-13-acp-claude-codex`（已把 Claude/Codex 适配器引擎接入，通用
ACP 路径可用）。研究依据：`../../10-13-acp-claude-codex/research/codeg-acp-stream-ui.md`
（codeg 全量盘点，含 codeg 路径+行号）。

## Goal / 用户价值

当前项目对 ACP 引擎响应只处理「通用流式文本 + 权限请求 + configOptions」三件套。当引擎
（尤其 codex/claude 适配器）发出**非纯文本**交互时，行为不完整甚至静默失败：

- codex Plan 模式 `request_user_input`、MCP 工具审批表单 → **静默失败**（我们 `-32601` 拒了
  `elicitation/create`）。
- 权限卡只有选项名，缺 `_meta.permission.changes[]`（选中会改什么）与 diff 预览。
- 并发权限请求无队列语义，看起来像卡死。
- 空回复失败、可重试错误、`set_config` 被 agent 改值等，前端无区分。
- `session/update` 的 plan/thought/tool_call_update 累积语义等未消费。

本父任务把这四类能力做齐，使 Claude/Codex/opencode 在对话框里的交互体验与 codeg 对齐
（**不含认证**——认证继续走厂商 CLI 自管凭据）。

## 范围（用户已确认）

**做**（4 个子任务）：
1. **AI 提问 / elicitation**（P0）——`elicitation/create` 分类与提问卡/审批桥接。
2. **权限授权富化**（P1）——`_meta.permission.changes[]` + diff 预览 + 审批队列事件。
3. **流中断 / 停止 / 重连**（P2）——cancel 语义、空回复失败推断、重试横幅、重连。
4. **`session/update` 全量种类消费**（P3）——plan/thought、`ConfigOptionRejected`、
   `TurnRetrying`、`session_info_update.title`、工具调用累积更新。

**不做**（明确排除，用户确认）：
- **认证**（`auth`/`login`/`authenticate`）——厂商 CLI 自管凭据，ACP 层不做专门 auth 流程。
- AIR 重型机制：steering / goal / `sessionFailure` / `contextCompaction` 卡 / fork /
  子代理委派 / background tasks / 每引擎 DB agent_setting / codex `apply_codex_env_policy`。
- 继续不自带/不下载适配器（沿用 `10-13-acp-claude-codex` D1）。

## 子任务映射（非依赖系统；排序见各子任务 prd）

| # | 子任务 | 目录 | 优先级 | 交付 |
|---|---|---|---|---|
| 1 | AI 提问 / elicitation | `10-14-acp-elicitation` | P0 | 提问/审批从 `elicitation/create` 分流并渲染 |
| 2 | 权限授权富化 | `10-14-acp-permission-rich` | P1 | 权限卡 changes/diff + 队列事件 |
| 3 | 流中断 / 停止 / 重连 | `10-14-acp-stream-control` | P2 | cancel/失败推断/重试/重连 |
| 4 | `session/update` 全量种类 | `10-14-acp-update-kinds` | P3 | plan/thought/config-rejected/retrying/标题 |

实施顺序按优先级（P0→P3）。子任务间若需依赖，写进各自 prd/implement（本父任务不建依赖系统）。

## 跨子任务 Acceptance Criteria

- [ ] **PAC1** 协议路由不含引擎名分支（延续 AC8）：引擎差异只以**数据/处理器注册**表达，
      不出现 `if agent == claude/codex`（`rg` 校验，命中仅允许在 `#[cfg(test)]`）。
- [ ] **PAC2** 所有新增 server→client 请求处理器在**未 advertise 时**走既有 `-32601`
      拒绝路径（不静默 hang）；已 advertise 时能正确应答。
- [ ] **PAC3** 新事件全部携带 `connId`（沿用 per-session 路由不变量），前端按 `connId`
      路由；不得引入全局（跨会话）状态。
- [ ] **PAC4** opencode 会话行为零回归（现有文本/工具/权限路径逐字节不变）。
- [ ] **PAC5** 每个子任务 `cargo check` + `cargo test --lib` + `vue-tsc --noEmit` +
      `pnpm build` 全绿；Rust 新增分支有单测（纯函数化）。
- [ ] **PAC6** spec 同步（`.trellis/spec/frontend/tauri-bridge.md` 及必要时新增子文档）。
- [ ] **PAC7** 认证相关不在任何子任务实现（`rg -n "auth|login|authenticate"` 新增代码无
      认证流程）。

## 集成验收（父任务，人工/实机）

- [ ] Claude 会话：Plan 提问（若有）与权限审批完整可答（AC5 扩展）。
- [ ] Codex 会话：Plan `request_user_input` 弹提问卡并可答；MCP 审批走权限卡。
- [ ] opencode 会话：全部既有能力不回归。

## Out of Scope（父任务级）

- 认证流程；AIR 重型机制（steering/goal/sessionFailure/contextCompaction/fork/子代理/
  background）；适配器打包下载；用户自定义引擎；每引擎 DB 表。
