# 研究：codeg 对话框对 ACP 引擎响应流的特殊处理（只读）

> 来源：`H:\AI\Light-vibe\codeg`（只读参考）。本文件盘点 codeg 在**聊天 UI 层**处理的所有
> 「不仅是流式文本」的 ACP 交互（server→client 请求 + `session/update` 种类 + 错误/重试/
> 认证），并对照本项目（lightr-vibe）现状给出可采纳项。供 implement/check 子代理核对。
>
> 本项目现状（基线）：`src-tauri/src/acp.rs` 只处理 `session/update` 的通用内容块、
> `session/request_permission`、`configOptions`；其余 client 请求（`fs/*`、`terminal/*`、
> `elicitation/create` 等）一律 `-32601` 拒绝。前端消费面见 `composables/useConversation.ts`。

## 1. 客户端注册的 server→client 请求处理器

codeg 用 `on_receive_request!` **逐方法**注册（`src-tauri/src/acp/connection.rs` ~6000–6250），
不是笼统拒绝。完整清单：

| 方法 | codeg 行为 | 证据 | 对我们的意义 |
|---|---|---|---|
| `session/request_permission` | `handle_permission_request` → 入队 → 弹授权卡 | `connection.rs:6000,8169` | 我们已有 |
| `fs/read_text_file` / `fs/write_text_file` | 真实现（`file_system_runtime`）；未声明 launch 走 `refuse_unadvertised_channel`（-32601） | `connection.rs:6050,6065,8301` | 我们 -32601 |
| `terminal/create` / `terminal/output` / `terminal/wait_for_exit` / `terminal/kill` / `terminal/release` | 真实现（`terminal_runtime`）；`wait_for_exit` 用 `cx.spawn` 回答（避免冻结单 dispatch 循环） | `connection.rs:6080,6095,6110,6153,6168` | 我们 -32601 |
| `elicitation/create`（codex） | `handle_elicitation_request` → `classify_elicitation` 分类为 Approval（走权限卡）/ Questions（走提问卡） | `connection.rs:6218,7951` | 我们 -32601（**高风险缺口**） |
| grok `_x.ai/ask_user_question`（ext request） | `handle_grok_ask_user_question` → 互动提问卡 | `connection.rs:6186` | 我们无 |
| grok `exit_plan_mode`（ext request） | `handle_grok_exit_plan_mode` | `connection.rs:6207` | 我们无 |
| grok 原生提问（`GrokAskUserQuestionRequest`）/ pi 的 select 提问 | 经 `try_bridge_pi_select_ask` 桥接进同一提问卡 | `connection.rs:6017` | 我们无 |
| 未声明通道 | `-32601` + `data` 说明（"not host <method> for this agent"） | `connection.rs:8315` | 我们同 |

要点：codeg 把 `fs/*` + `terminal/*` 作为 **host 工具通道**，由 launch 策略决定是否 advertise
（`HostToolsPolicy`），未声明才拒绝——不是无脑拒绝。

## 2. 权限授权（`session/request_permission`）

- **队列化、一次一张卡**：`PendingPermissions` / `admit_permission`（`connection.rs:18226`）。
  事件三件套（`types.rs:427–457`）：
  - `PermissionRequest { request_id, tool_call, options, queued }`（`queued` = 其后排队数）
  - `PermissionQueueDepth { depth }`（屏上卡不变但队列变化时更新计数）
  - `PermissionResolved { request_id }`（用户已答/连接排空/agent 撤回）
  没有后两个事件，并发审批在 UI 上看起来像「卡死」。
- **选项 kind 映射**：`allow_once` / `allow_always` / `reject_once` / `reject_always`
  （`connection.rs:8219–8225`），未知 → `"unknown"`。
- **`_meta` 富信息**（`air_contract::normalize_permission_option_meta`，`connection.rs:8230`）：
  - 请求级 `_meta.permission.{title,description,defaultToNo}`（`src/lib/permission-request.ts:33–50`）
  - 选项级 `_meta.permission.changes[]`：选中该选项**会改什么**（含 description），
    前端渲染成「会变更的文件/范围 + persistent 徽章」；上限条数防刷屏（`permission-request.ts:949–1090`）
  - **diff 预览**：对 edit 类工具的 `rawInput` 注入 `_start_line` / `resolve_patch_text`，
    前端 `UnifiedDiffPreview` 直接画 diff（`connection.rs:8236–8264`、`permission-dialog.tsx:176`）
  - Codex Plan-mode 审核：`codex_plan_review_meta` 预置一张 `switch_mode` 待定工具卡，
    让后续只有 status/rawOutput 的 `tool_call_update` 有身份可合并（`connection.rs:8188–8211`）
- **自动放行例外**：codeg 自己 ask 工具的审批（"为显示对话框而请求权限"）自动 allow
  （`try_auto_allow_codeg_ask`，`connection.rs:6003`）。

## 3. AI 提问 / elicitation（`src-tauri/src/acp/question.rs`）

- **`ask_user_question`** 是 codeg 自注册的 MCP 工具（`mcp__codeg-mcp__ask_user_question`）；
  Rust 侧**阻塞等用户回答**（`question.rs:4`）；一次最多 `MAX_QUESTIONS`（`question.rs:42`，仿 Claude Code）。
- **`classify_elicitation`**（`question.rs:1255`）把 `elicitation/create` 分类：
  - **Approval 型**（MCP 工具调用审批 / message-only confirm）→ 复用**权限卡**（不产出提问卡）；
    这是 codeg advertise `elicitation.form` 之前 codex 走 `session/request_permission` 的同一界面
    （`connection.rs:8001–8055`）。
  - **Questions 型**（codex Plan `request_user_input`、通用 MCP form）→ 桥接进**提问卡**
    （`connection.rs:8056–8080`）。
  - 支持 single/multi-select、free-text、boolean、enum（显示 title、回传 const）、
    `none_of_the_above` 自定义答案；URL elicitation 转权限卡；无法渲染的类型 → `decline()` 让 agent 自行判断。
- **能力协商**：只对**已知会发规范 form** 的 agent advertise `elicitation.form`
  （codex、DeepSeek 等；`connection.rs:4753–4764`），避免误触发未知 MCP 表单。
- **总开关**：`ask_user_question` 可被用户关闭 → 直接 `decline`（`question.rs:1787 is_enabled`）。
- **前端组件**：`QuestionDialog`（单文本答案，`chat/question-dialog.tsx:17`）、
  `AskQuestionCard`（多选待答，`chat/ask-question-card.tsx`）、
  `ask-question-result-card`（把答案回填进消息流，`message/ask-question-result-card.tsx`）。
- **多来源桥接**：claude 的 codeg-mcp ask、codex 的 elicitation form、grok 的 ext request、
  pi 的 select —— 全部收敛到同一套「提问卡」UI + 同一 Rust 阻塞问答设施。

## 4. `session/update` 种类 → 前端事件（`src-tauri/src/acp/types.rs:366` `enum AcpEvent`）

本项目只认文本块；codeg 的完整事件表（节选，标注前端渲染）：

- **内容**：`ContentDelta`、`Thinking`（二者都可带 `parent_tool_use_id` → 子代理线程分流）
- **工具**：`ToolCall`、`ToolCallUpdate`（`raw_output_append` 累积语义、`locations`、`images`、`meta`）
- **计划**：`PlanUpdate { entries }` → 前端 `PlanCard` / `plan-mode-card` / `plan-approval-card`
- **模式/配置**：`SessionModes`、`ModeChanged`、`SessionConfigOptions`、
  **`ConfigOptionRejected`**（agent 把 set_config 落成别的值时给提示；否则下拉"无故弹回"）
- **会话元信息**：`SessionStarted`、`NativeSessionTitle`、`SelectorsReady`、`PromptCapabilities`、`ForkSupported`
- **错误/重试**：`Error { message, code?, details? }`（`code` 给前端本地化；`details` 是脱敏的 stderr tail）、
  **`TurnRetrying`**（`_meta.codex.error` willRetry / pi auto_retry → 重试横幅，**不是失败**）
- **AIR 重型（本项目 Out of Scope）**：`SessionFailure`、`SessionNotice`、`ContextCompaction`、
  steering、goal、delegation、background tasks（`types.rs:640–710+`）

前端后处理（`src/lib/adapters/ai-elements-adapter.ts`）：`<proposed_plan>` 提取为计划卡、
计划文本去重、纯文本 badge、工具调用归一化、`context-compaction` 卡等。

## 5. 流中断 / 停止 / 重连

- `TurnComplete { stop_reason, ... }`（`types.rs:459`）。
- cancel 后**下发该会话进行中的权限卡**并 abort（`connection.rs:3198`）。
- **静默失败推断**：agent 报成功但 wire 无内容 → `turn_failed_empty*` 错误族，靠 stderr tail 兜底
  （`Error.details`，`types.rs:570–584`）。
- 重试横幅（`TurnRetrying`）与失败卡（`Error`）分离。

## 6. 认证 / 登录态

- 主要靠**各厂商 CLI 自管凭据**：`~/.grok/auth.json`、`~/.claude`、`~/.codex`；
  codeg 只在**设置页**给登录命令/设备码卡片（`grok login`、`cline auth -p` 设备码流程，
  `components/settings/acp-agent-settings.tsx:691,726`）。
- **ACP 层没有专门的 `auth_required` 通知被消费**——凭据问题在 prompt 时以普通 `Error` 暴露。
- Antigravity 有专门 OAuth/token 管理（`antigravity_login.rs`）——本项目不涉及。

## 7. 采纳候选（lightr-vibe）

按价值排序。所有项均**不需要**触碰协议路由的引擎名（延续 AC8：差异只在数据/处理器注册）。

| 优先级 | 项 | 最小协议面 | 备注 |
|---|---|---|---|
| **高** | `elicitation/create` 处理 | 注册 `elicitation/create` 请求处理器；`classify_elicitation` 的 Approval/Questions 二分类（可先只做 Questions 子集） | codex Plan `request_user_input`、MCP 表单现在会**静默失败**；claude/codex 适配器都会发 |
| **高** | 权限卡的 `_meta.permission.changes[]` + diff 预览 | 前端读 `option.meta.permission.changes`；Rust 透传 `_meta`（已可透传） | 我们现在只显示选项名；codex/claude 的 edit 审批缺 diff |
| **中** | `PermissionQueueDepth` / `PermissionResolved` 事件 | Rust 发两个补充事件；前端显示"还有 N 个待审批" | 并发审批现在看起来像卡死 |
| **中** | `TurnRetrying` | 解析 `session_info_update._meta.codex.error`（willRetry）→ 前端重试横幅 | 现在会被误当失败或丢失 |
| **中** | `ConfigOptionRejected` | set_config 后比对 agent 落定值 → 提示 | 否则模型/模式下拉"无故弹回" |
| **低** | `fs/*` / `terminal/*` 通道 | 真实现（`fs/read_text_file` 等）或维持 -32601 | 现在 -32601 已可用；host 工具是可选增强 |
| **低** | `PlanUpdate` → 计划卡 | 前端消费 `plan` 更新种类 | 现在计划以普通文本流显示 |
| **低** | `session_info_update.title` → 侧栏标题 | 已有 `applyAutoTitle` 走本地逻辑；可改用 agent 原生标题 | 增强项 |

**明确 Out of Scope（照 prd.md）**：steering、goal、AIR `sessionFailure`、
`contextCompaction` 卡、fork、子代理/委派、background tasks、每引擎 DB agent_setting、
codex `apply_codex_env_policy`。这些是 codeg 的重型机制，本期不实现。

**依赖顺序建议**：先做 `elicitation/create`（Questions 子集）+ 权限卡 diff 预览（二者
直接提升 claude/codex 可用性，验证 AC5）；`TurnRetrying` / `ConfigOptionRejected` 次之；
`fs/*` / `terminal/*` 与 `PlanUpdate` 视后续需求。
