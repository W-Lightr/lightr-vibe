# Implementation Plan：子任务统一委派与右侧栏子任务页签

## Preconditions

- 任务保持 `planning`，直到本计划的评审闸门通过并执行 `task.py start`。
- 工作树中的其它改动（`scripts/smoke-lightr-ask-mcp.ps1`、`tauri-bridge.md`）不得回退。
- 不引入新依赖；不修改委派 wire 契约。

## Step 1 · 工具描述引导（Rust）

- [ ] 修改 `src-tauri/src/bin/lightr_ask_mcp.rs` 的 `delegation_tool_defs()`：
  - `delegate_to_agent.description` 增加「本应用内子任务的唯一受支持入口；不要使用引擎自带的
    子代理 / task 工具，否则用户看不到子任务进度面板」。
  - `get_delegation_status` / `cancel_delegation.description` 注明操作对象。
- [ ] 不改 `inputSchema` / socket 协议 / 引擎启动参数。
- [ ] `src-tauri/src/acp.rs` 新增 `with_session_meta()` + `DELEGATION_SYSTEM_HINT`，对
  `AgentType::ClaudeCode` 且委派开启时，给 `session/new`、`session/load`、
  `run_delegated_child` 的参数加 `_meta.systemPrompt.append`。
- [ ] 不注入 `disallowedTools`（保留 Claude 原生 Agent 工具，软引导）。
- [ ] 校验：`cargo check` 通过；直接探针 `initialize` + `session/new`（带 `_meta.systemPrompt`）
  返回 sessionId；重新构建后 `pnpm smoke:ask-mcp` 仍列出 4 个工具。

**Review gate 1**：描述文本确认无歧义、无夸大（不承诺强制拦截）。

## Step 2 · 页签状态与子任务派生（前端状态层）

- [ ] `src/stores/workspace.ts`：
  - 导出 `RightTab` 类型，新增 `rightTab` ref（默认 `'files'`）。
  - `openDelegatedChild()` 同时设置 `rightTab = 'subtasks'` 与 `inspectorVisible = true`。
  - 新增 `closeInspector()`；保留 `closeDelegatedChild()` 语义为「回到列表」。
- [ ] 新增 `src/utils/delegationStatus.ts`：`delegationStatusText` / `delegationStatusClass` /
  `isWaitingStatus`，与 `DelegationCard.vue` 现有文案、配色一致。
- [ ] `DelegationCard.vue` 改用上述工具函数（行为不变）。
- [ ] 校验：`pnpm build`（`vue-tsc --noEmit`）通过。

**Review gate 2**：确认没有其它代码依赖 `workspace.delegatedChildSessionId` 的旧渲染位置。

## Step 3 · 右栏页签重构

- [ ] `RightPanel.vue`：
  - 页签改用 `workspace.rightTab`；固定三项 + 条件项 `subtasks`（label 带计数）。
  - 计算 `childSession` 与 `hasSubtasks`；页签不可用时回落到 `files`。
  - 分支渲染 `DelegatedChildView` / `DelegationTaskList` / 既有内容。
- [ ] `CodingWorkspace.vue`：删除右栏子会话整块替换逻辑，`RightPanel` 保持唯一入口。
- [ ] 校验：`pnpm build`；浏览器 dev 下文件 / 变更 / 详情页签行为不变。

**Review gate 3**：页签可见性、计数、回落逻辑符合 PRD R2 / R6 / AC2 / AC7。

## Step 4 · 子任务列表与卡片

- [ ] 新增 `src/views/coding/components/DelegationTaskList.vue`：
  - props `parentSessionId`；过滤 + 按 `createdAt` 倒序；空态文案。
- [ ] 新增 `src/views/coding/components/DelegationTaskCard.vue`：
  - 状态徽标、引擎、任务摘要（单行截断 + title 全文）、耗时、短 id。
  - 点击 → `workspace.openDelegatedChild(childSessionId)`；child session 缺失时置灰。
- [ ] 校验：`pnpm build`；浏览器 mock 下无记录 → 页签不渲染、无报错。

**Review gate 4**：卡片信息完整、无重复状态逻辑（第三次拷贝视为不通过）。

## Step 5 · 子会话详情与返回

- [ ] 新增 `src/views/coding/components/DelegatedChildView.vue`：
  - 顶部「← 返回」（`closeDelegatedChild`）、标题、「×」（`closeInspector`）。
  - `ConversationView` + `MessageComposer passive`，布局照搬现抽屉。
- [ ] 校验：`pnpm build`；手动走通列表 → 详情 → 返回 → 关闭右栏 → 重开。

**Review gate 5**：详情滚动、权限 / 提问队列、输入框禁用态与现抽屉一致。

## Step 6 · 端到端联调

- [ ] 桌面 Tauri：设置 → Agent 管理 → 开启「子代理委派」→ 新建会话。
- [ ] 让模型执行一个子任务，确认：
  - 模型选择 `delegate_to_agent`（工具名与参数为 `agent_type` / `task`）；
  - 右栏自动切到「子任务」页签并展示详情；
  - 返回列表后卡片状态随完成实时更新；
  - 父会话委派卡仍可打开子会话。
- [ ] 切换会话 / 无子任务会话 / 关闭右栏重开，验证页签可见性与回落。
- [ ] 重启应用，确认 `interrupted` 记录出现在列表且可进入详情。

## Step 7 · 质量门禁与收尾

- [ ] `pnpm build`
- [ ] `cd src-tauri && cargo check`
- [ ] 相关 Rust 测试（delegation / companion MCP）
- [ ] `pnpm smoke:ask-mcp`
- [ ] 更新 `.trellis/spec/frontend/tauri-bridge.md`：记录「子任务页签 ↔ delegation://started」和
  工具描述引导的约定（若实现中形成新契约）。
- [ ] 回填任务验证结果，提交前确认未回退工作树中的既有修复。

## Risk Checkpoints / Rollback

| Checkpoint | 风险 | 止损 |
|---|---|---|
| Step 2 | 页签状态提升影响 `openDelegatedChild` 既有调用方 | `pnpm build` + 手动打开委派卡验证；异常则回退 store 改动 |
| Step 3 | 删除整块替换逻辑后右栏空白 / 高度塌陷 | 保留旧分支注释在 diff 中，可快速恢复 |
| Step 4 | 状态文案与 `DelegationCard` 分叉 | 强制共用 `delegationStatus.ts`，代码评审拦截 |
| Step 6 | 工具描述引导无效，模型仍用原生子代理 | 记录实测结果；本任务不引入引擎侧禁用，后续单独立项 |

## 验证记录（2026-11-10，主会话自行实现）

| 检查 | 命令 | 结果 |
|---|---|---|
| 前端类型 + 构建 | `pnpm build` | ✅ 通过 |
| Rust 编译（全量） | `cd src-tauri && cargo check` | ✅ 通过 |
| companion 重建 | `cargo build --bin lightr-ask-mcp` | ✅ 通过 |
| tools/list 冒烟 | `pnpm smoke:ask-mcp` | ✅ 4 个工具全部列出 |
| 工具描述生效 | 直接跑 companion `tools/list` | ✅ 新描述已进入二进制 |
| Claude `_meta` 探针 | `node claude-agent-acp` + `initialize` + `session/new`（带 `_meta.systemPrompt`） | ✅ 接受并返回 sessionId |
| companion 描述重建 | MSVC 环境下 `cargo build --bin lightr-ask-mcp` + `pnpm smoke:ask-mcp` | ✅ 4 个工具；625 字新描述进入二进制 |
| 重复卡清理 | 扫描 `~/.lightr-vibe/lightr-vibe/sessions/*.json` | ✅ 合成卡已删除，只剩真实 MCP 调用 |
| 思考/正文串流修复 | `pnpm build` | ✅ 通过（`useConversation` 本地消息 id 加内容类型前缀） |

### 附带修复 2：重复的委派卡

每个委派在 transcript 里出现两次：adapter 回传的真实 MCP 调用
（`mcp__lightr-ask__delegate_to_agent` / `lightr-ask_delegate_to_agent`）与
`ensureDelegationCard` 生成的合成卡（`delegation:<taskId>`）。

修复：

- `ensureDelegationCard` 先探测真实调用（优先按返回 `taskId`，运行中退化为 `task` + `agent_type`），
  命中则不再建合成卡，并删除历史遗留的合成消息；启动恢复 `projectDelegation` 走同一路径。
- `ToolCallPart.isDelegation` 的匹配从 `(?:^|__)` 放宽到 `(?:^|[_-])`，让
  `lightr-ask_delegate_to_agent`（opencode 的 title 形态）也渲染成委派卡。
- 实测：重载后各会话 JSON 里合成的 `delegate_to_agent` 消息已被清理，只剩真实 MCP 调用。

### 附带修复 1：思考/正文串流

Claude 会话里最终回答渲染进「思考」块：`claude-agent-acp` 给同一条 assistant 消息的
`agent_thought_chunk` 与 `agent_message_chunk` 打同一个 ACP `messageId`，前端直接拿它当本地
消息 id，正文 chunk 命中了先创建的思考消息并被 append 进去。实测证据：原始记录
`thinking 77 + text 1307`，落盘成一条 `thinking 1384` 且 `text` 为空。

修复：`useConversation.applyUpdate` 的本地消息 id 改为 `<type>:<messageId>`。历史已落盘的
消息不回填；新 turn 起恢复正常。

实现说明：

- 工具描述改写位于 `lightr_ask_mcp.rs::delegation_tool_defs()`，`inputSchema` 未动。
- 页签状态提升到 `workspace.rightTab`；`openDelegatedChild` 自动切页签，`closeInspector` 保留页签。
- 新增 `DelegationTaskList.vue` / `DelegationTaskCard.vue` / `DelegatedChildView.vue` /
  `utils/delegationStatus.ts`；`DelegationCard.vue` 改用共用状态工具。
- 子任务卡「耗时」：终态用 `updatedAt`，运行中按当前时间计算，避免显示 0 秒。
- 规格已同步：`.trellis/spec/frontend/tauri-bridge.md` 新增「Subtask tab + delegation guidance」。

尚未执行（需真实桌面会话人工验证）：AC1（模型实际选用 `delegate_to_agent`）、AC3/AC4/AC5/AC6
（列表→详情→返回、自动打开、右栏重开）、AC7（切会话）、AC8（状态实时刷新）、AC9（浏览器 mock 渲染）。

## Final Quality Gate

- [x] `pnpm build`
- [x] `cd src-tauri && cargo check`
- [x] `pnpm smoke:ask-mcp`
- [ ] 桌面手动流程（列表 / 详情 / 返回 / 自动打开 / 会话切换 / 重启恢复）
- [ ] 现有单代理会话、权限、提问、文件编辑无回归
