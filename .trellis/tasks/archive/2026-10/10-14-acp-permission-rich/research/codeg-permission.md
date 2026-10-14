# 研究：codeg 的 ACP 权限授权实现（只读）

> 来源：`H:\AI\Light-vibe\codeg`（只读参考）。本文件盘点 codeg 对
> `session/request_permission` 的完整处理，供本项目（lightr-vibe）「权限授权富化」子任务采纳。
> 证据均为 codeg 路径 + 行号。
>
> 本项目现状：`src-tauri/src/acp.rs::handle_client_request` 特判 `session/request_permission`
> → 存 `conn.pending_perm[request_id] = (rpc_id, sessionId)` → emit
> `acp://permission {connId, requestId, sessionId, toolCall, options}`；前端
> `composables/useAcpPermission.ts`（队列 + auto/plan/confirm 三态策略）+
> `views/coding/components/PermissionCard.vue`（允许一次/本会话总是允许/拒绝）。

## 1. 权限请求生命周期（Rust，`src-tauri/src/acp/connection.rs`）

- **一次一张卡的队列**：`PermissionQueue`（`:2806`）持 `showing: Option<...>` + `waiting:
  VecDeque<QueuedPermission>`（`:2754,2810`）。`admit()`（`:2850`）无人在屏则直发，否则入队；
  `promote_next()`（`:2919`）在当前卡离开时把队首推上屏。
  - **为什么必须队列**（`:2760-2799` 长注释）：卡片在快照与前端 reducer 里都是**单槽**，
    N 个并发请求会互相覆盖 → 早先的 `request_id` 不再被任何客户端引用 → 其 responder 永远
    挂着 → agent 工具调用**永久阻塞**（work task 卡在 `awaiting_input`）。codex 会并发发
    （每个 `item/commandExecution/requestApproval` 转成独立 `session/request_permission`）。
  - `PendingPermissions` 与队列**共用一把锁**（`:2760` 起），emit 在锁内完成
    （`admit_permission` `:3003`），使「responder 存在」与「卡在屏」相对 drain 原子。
- **`PermissionReply` 枚举**（`:2646`）：`Acp(Responder<RequestPermissionResponse>)` |
  `CodexElicitation{responder, approval}`（审批型 elicitation 复用同一张卡）。
  `respond_selected(option_id)` / `respond_cancelled()`（`:2692-2726`）。
- **`QueuedPermission`**（`:2736`）= 恰好 `AcpEvent::PermissionRequest` 的三字段
  `{request_id, tool_call, options}`，promote 是移动而非重建。
- **事件三件套**（`types.rs:427-457`）：
  - `PermissionRequest{request_id, tool_call, options, queued}`（`queued` = 其后排队数）
  - `PermissionQueueDepth{depth}`（屏上卡不变、队列变化时更新计数）
  - `PermissionResolved{request_id}`（answered / drained / withdrawn）
  emit 顺序见 `resolve_permission`（`:3093`）与 `:3157`（先 promote 的 `PermissionRequest`
  再 `PermissionResolved`）。
- **自动放行例外**：`try_auto_allow_codeg_ask`（`:7472`，放行「为显示对话框而请求权限」）；
  `try_bridge_pi_select_ask`（`:6017`，pi 借权限通道问问题 → 转提问卡）。

## 2. 选项语义（`handle_permission_request`，`:8169`）

- `PermissionOptionInfo{option_id, name, kind, meta}`（`:8213`）；kind 映射：
  `allow_once` / `allow_always` / `reject_once` / `reject_always` / 其他 → `unknown`（`:8219`）。
- **`_meta` 归一化**：`crate::acp::air_contract::normalize_permission_option_meta(opt.meta)`
  （`:8230`）——接受 codex-acp ≥1.1.8 的 `_meta.permission.changes[]`，并兼容 codex-acp 2.0.0
  移到 `_meta.jetbrains.air` 的记录（拷回卡读取的键）。
- **edit 审批的行号/diff 解析**（`:8234-8264`）：对 `tool_call.rawInput`（object 或 JSON
  字符串）注入 `_start_line`（`inject_start_line(v, Some(cwd))`）；字符串且是 patch（含 `@@`）
  → `crate::parsers::resolve_patch_text(&text, Some(cwd))`。
- `hoist_request_permission_meta(&mut tool_call, req.meta)`（`:8266`）：把请求级 `_meta`
  提到前端读的位置。

## 3. `_meta.permission` 富数据（`src/lib/permission-request.ts`）

- **选项级 `changes[]`**：`parsePermissionOptionChanges(meta)`（`:1075`）：
  - 两形态，都在 `PermissionOption.meta.permission`：
    - `{version:1, changes:[{description, lifetime?}]}`（claude-agent-acp 0.64.1–0.72.0 /
      codex-acp 1.1.8–1.6.2）
    - `{version:1, description}`（codex-acp ≥1.7.0，单条无 scope）
  - 输出 `PermissionOptionChange{description, scope}`；`scope` 从 `change.lifetime`
    （`{scope:"session"}` vs `{scope:"persistent", storage:"project"}`）归一化。
  - 上限 `MAX_PERMISSION_CHANGES` / `MAX_PERMISSION_CHANGE_CHARS`。
  - **注**：claude-agent-acp ≥0.73 把变更直接写进按钮 name，选项不再带 `_meta` → 实际多为空。
- **请求级**：`_meta.permission.{title, description, defaultToNo}`（`:33-50,1015-1031`）；
  `_meta.claudeCode.title` 作为 title 回退（`:873-885`）。
- `defaultToNo`（claude-agent-acp ≥0.77）标记「不该被误触批准的请求」→ 前端把强调色给
  **拒绝** 按钮（`permission-dialog.tsx:346-355`）。

## 4. 权限卡 UI（`src/components/chat/permission-dialog.tsx`）

- 头部：`ShieldAlert` + 描述（`parsed.description ?? parsed.title`）+ 理由行 + `queued` 徽章
  （`t("queuedCount")`, `:149-153`）+ kind 徽章（`:154-156`）。
- 身体分区块（`:160-344`）：command（`CodeBlock`）、**diff（`UnifiedDiffPreview`）**、
  plan（entry 列表 / markdown）、web（url/query）、**选项 grants
  （`_meta.permission.changes`，`KeyRound` 图标，`:286-331`）**、contentText、raw-JSON 兜底。
- 选项按钮（`:352-367`）：按 option `kind` 区分 reject；`defaultToNo` 时强调 reject。
- 挂载：`conversation-shell.tsx:245` `permission={pendingPermission}`。

## 5. 与 elicitation 的耦合

- 审批型 `elicitation/create`（MCP 工具调用审批 / message-only confirm）→ 复用**权限卡**
  （`handle_elicitation_request` `:8007-8055`，构造 `buildPermissionRequest` 形状的 tool_call
  + `PermissionOptionInfo`，走 `admit_permission`）。本项目本期不做 elicitation（独立子任务）。

## 6. 采纳候选（lightr-vibe「权限授权富化」）

按价值排序；标注「重/不适用」。本项目**不 host `fs/*`/`terminal/*`**（-32601），故忽略依赖
host channel 的部分。

| 优先级 | 项 | 最小协议面 | 备注 |
|---|---|---|---|
| **高** | 选项级 `_meta.permission.changes[]` 渲染 | Rust：**已透传 options 原样**（含 meta，因为我们 `params.options` 整体 `cloned()`）→ 前端解析 `option.meta.permission.changes` + `description`，卡上「选中会变更什么」区块 | 现在只显示选项名 |
| **高** | 请求级 `_meta.permission.{title,description,defaultToNo}` + `_meta.claudeCode.title` | 前端解析 `toolCall._meta`（Rust 需把请求级 `_meta` 提到 toolCall 或事件里）；`defaultToNo` 反转强调色 | 现在标题就是命令 |
| **高** | edit 审批 diff 预览 | 前端：`toolCall.rawInput` 的 `_start_line`/patch → diff 视图。Rust 需注入 `_start_line`（`inject_start_line`）或前端解析 rawInput | 现在只显示 JSON 摘要 |
| **中** | 审批队列深度 `queued` + `PermissionQueueDepth` + `PermissionResolved` | Rust：`pending_perm` 改队列（单槽 + waiting），emit 三个事件；前端显示「还有 N 个待审批」 | 现在队列在前端、无深度事件；并发多请求 UI 只见一张且无计数 |
| **中** | 选项 `kind` 完整映射 + 按 kind 选按钮（而非硬编码三按钮） | 前端按 `options` 动态渲染（reject_always 等新 kind） | 现在硬编码 允许一次/总是/拒绝 三按钮 |
| 低 | `defaultToNo` 强调反转 | 见上 | |
| — | 审批型 elicitation 复用权限卡 | **不适用**：属 elicitation 子任务 | |
| — | `try_auto_allow_codeg_ask` / pi 桥接 | **不适用**：codeg 自注册 MCP / pi 专属 | |
| — | AIR `_meta.jetbrains.air` 归一化 | 低/重 | 仅 codex-acp 2.0.0 记录迁移，可后置 |

**一句话**：最高价值 = ①选项 `changes[]` ②请求级 description/title/defaultToNo ③edit diff，
三者都是**前端解析 + Rust 少量透传/注入**，不涉及 host channel，风险低、收益直接。
队列深度事件是中等价值、需要动 Rust pending 结构。
