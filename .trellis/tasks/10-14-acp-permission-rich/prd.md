# 权限授权富化（changes / diff / 队列）

父任务：`10-14-acp-rich-stream-ui`（P1，本轮**单独优先**）。前置：`10-13-acp-claude-codex`
（引擎接入，含 claude/codex 权限走通）。研究依据：
`research/codeg-permission.md`（codeg 权限实现全量盘点，含路径+行号）。

## Goal / 用户价值

当前权限审批卡（`PermissionCard.vue`）只显示「工具名 + rawInput JSON 摘要 + 三个固定按钮」。
codex/claude 适配器在权限请求里携带**富信息**，我们全部丢弃：

1. **选项级 `_meta.permission.changes[]`**：「选中这个选项会授予什么权限 / 持续多久」——安全决策
   的核心依据。当前只显示选项名。
2. **请求级 `_meta.permission.{title, description, defaultToNo}` 与 `_meta.claudeCode.title`**：
   人读的请求标题与理由；`defaultToNo` 标记「不该被误触批准」的请求（应强调拒绝）。当前标题就是
   命令本身。
3. **edit 类审批的 diff 预览**：`rawInput` 里的补丁/行号可直接渲染成 diff；当前只显示 JSON 摘要。
4. **审批队列深度**：并发审批时前端只见一张卡、且无「还有 N 个待审批」提示 → 看起来像 agent 卡死。

本期让权限卡达到 codeg 的信息密度与决策质量（**不含** elicitation、AIR、host channel）。

## Resolved Decisions（用户已确认：单任务、全力保障权限）

- **D1 单任务聚焦**：本轮只做权限授权富化；elicitation / 流控制 / update 种类**暂不做**。
- **D2 不 host `fs/*` / `terminal/*`**：沿用现状 `-32601`。故所有依赖「由我们执行 fs/terminal」
  的 codeg 行为不做（`rawInput` 行号解析走**本地注入**而非 host channel）。
- **D3 队列深度事件为中等项**：若 Rust 改动面可控则做（`queued` 计数）；否则前端先以「队列长度」
  近似。目标仍是对齐 codeg 的「还有 N 个」语义。
- **D4 动态选项按钮**：由 `options` 的 `kind` 驱动渲染（不再硬编码三按钮），兼容
  `reject_always` 等。

## Requirements

### R1 前端：选项级 `_meta.permission.changes[]`
- 解析 `AcpPermissionOption.meta.permission`：`{version:1, changes:[{description, lifetime?}]}`
  与 `{version:1, description}` 两形态（照 codeg `parsePermissionOptionChanges`）。
- 卡上新增「选项授予内容」区块：每个选项列出其 changes（含 scope 徽章：会话内 / 持久）。
- 上限条数/字符（仿 codeg），防止刷屏。

### R2 前端 + Rust：请求级 `_meta`（title / description / defaultToNo）
- Rust：`acp://permission` 事件增加 `meta`（`params._meta`，原样透传；缺失 → null）。
- 前端：`title` = `_meta.claudeCode.title ?? _meta.permission.title ?? toolCall.title`；
  `description` = `_meta.permission.description`；`defaultToNo` → 强调色给**拒绝**按钮。
- types：`AcpPermissionEvent.meta?: unknown`；`AcpPermissionOption.meta?: unknown`。

### R3 前端：edit 类审批 diff 预览
- 若 `toolCall.rawInput` 含补丁（`@@`）或行号信息，渲染为 diff 视图（复用/新建统一 diff 组件）。
- Rust：对 `rawInput` 注入 `_start_line`（照 codeg `inject_start_line`）——**仅在是对象/可解析
  时**，失败保持原样（best-effort，不阻断）。
- 无 diff 时回退现有 JSON 摘要。

### R4 前端：动态选项按钮（D4）
- 由 `options[].kind` 渲染按钮；文案优先 `option.name`（agent 给），无则按 kind 回退中文。
- 保留「本会话总是允许」语义：命中 `allow_always` 才记入 always-allowed map。

### R5 队列深度（D3）
- Rust：`acp://permission` 增 `queued` 字段（该会话其后排队数）+ 需要时 `acp://permissionResolved`
  / 深度事件；前端卡上显示「还有 N 个待审批」。
- 若本轮判定 Rust 队列重构风险过大 → 降级为：前端按 `queue.length` 显示计数（标注近似），
  Rust 侧留 TODO。**在 design.md 定稿取舍**。

## Acceptance Criteria

- [ ] **AC1** `cargo check` + `cargo test --lib` + `vue-tsc --noEmit` + `pnpm build` 全绿。
- [ ] **AC2** 选项带 `_meta.permission.changes[]` 时，卡上按选项列出 changes（含 scope 徽章）；
      无 `_meta` 时区块不出现（不报错）。
- [ ] **AC3** 请求级 `title/description` 正确取自 `_meta`（含 `claudeCode.title` 回退）；
      `defaultToNo` 使**拒绝**按钮为强调色。
- [ ] **AC4** edit 审批（rawInput 含补丁/行号）渲染 diff；无则回退 JSON 摘要。
- [ ] **AC5** 选项按钮由 `options` 动态渲染；点击回应正确 `optionId`；`allow_always` 记入
      always-allowed。
- [ ] **AC6** 并发多个待审批时，UI 显示待办计数（真实 `queued` 或队列长度近似，design 定稿）。
- [ ] **AC7** opencode 会话权限卡行为零回归（PAC4）：无 `_meta` 时分米级等价旧卡。
- [ ] **AC8** 协议路由不含引擎名分支（PAC1）。
- [ ] **AC9** 浏览器 mock：`acpMock` 提供一条带 `_meta` 的样例请求，UI 不崩溃。
- [ ] **AC10** 实机（Tauri，人工）：codex `Bash`/edit 审批卡显示 changes/diff；claude 审批卡
      正常（人工验收）。

## Out of Scope

- elicitation / 审批型 elicitation 复用权限卡（独立子任务）。
- `fs/*` / `terminal/*` host channel（D2，仍 `-32601`）。
- AIR `_meta.jetbrains.air` 全量归一化（仅保留最低兼容，见 design）。
- 认证。
