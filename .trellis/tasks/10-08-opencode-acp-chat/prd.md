# 子任务 2：OpenCode ACP 真实对话

父任务：`10-08-real-agent-workspace`。依赖：可独立先行（cwd 用 tmpsession
目录）；子任务 1 完成后会话可绑定真实项目 cwd。
协议基线：`research/acp-protocol.md`、`research/opencode-acp-probe.md`（实测
opencode 1.18.35）。

## Goal / 用户价值

对话区从 mock 事件流切换为 OpenCode ACP 真实 Agent：发送消息 → 流式回复 /
思考 / 工具调用面板真实内容 → 可停止 → 权限请求按执行模式处理 → 会话与消息
跨启动持久化并可继续。

## Confirmed facts（research 实测）

- 传输：newline-delimited JSON-RPC 2.0 over stdio（非 Content-Length）。
- 生命周期：initialize ~1.2s；`session/new` 首调 7.6-8.9s（模型枚举）→ 超时
  ≥30s；stdin 关闭 <1s 干净退出；stderr 默认静默。
- 模型：无 list_models；`session/new` 响应 `configOptions`（model 20 项含用户
  provider、mode=build/plan）；切换用 `session/set_config_option`。
- 流式：`session/update` notifications —— `agent_message_chunk`（带 messageId）、
  `tool_call`/`tool_call_update`（content **累积替换**语义）、`usage_update`、
  `available_commands_update` 等 11 变体。
- 取消：`session/cancel` notification → 44ms 回 `stopReason:"cancelled"`；
  idle 时 cancel 为安全 no-op；pending permission 需回 cancelled。
- 权限：client 侧 `session/request_permission`（选项列表含 optionId）。
- 无凭据：握手/建会话不报错，模型仅剩免费 Zen 且可对话；失败推迟到
  set_model(-32602) 或需凭据模型的 prompt 阶段。
- Windows：PATH 为 version-fox `.cmd` shim，真身
  `node_modules\opencode-ai\bin\opencode.exe`；长驻进程直 spawn 真身 +
  CREATE_NO_WINDOW，shim 解析回退。
- 既有桥接规范：tauri-bridge.md（camelCase、conflict、isTauri 双通道）。
- 数据命名空间：仅 `~/.lightr-vibe/lightr-vibe/`（父任务约束）。

## Resolved Decisions（用户确认）

- Q1 Agent 不可用：发送时引导提示跳设置页，**不假回复**。
- Q2 持久化：自存会话索引 + 消息落盘（重启恢复、可继续）。
- 权限 UX：**绑定执行模式** —— auto=自动允许；confirm=对话内审批卡（允许一次 /
  本会话总是允许 / 拒绝）；plan=只读工具放行、其余拒绝。切换即时生效。

## Requirements

### R1 ACP 进程管理（Rust src-tauri/src/acp.rs）
- 长驻单进程多会话：`acp_start`（解析 opencode 真身路径：which → shim 内容解析
  回退 → `cmd /C` 末选；CREATE_NO_WINDOW）→ initialize 握手 → status ready。
- stdout 行读取线程：JSON-RPC 分流 —— response 按 id 关联（pending map +
  每请求超时：initialize 10s / session/new 30s / prompt 300s / 其余 30s）；
  notification → `app.emit("acp://update", {sessionId, update})`；
  client 侧 request（request_permission / fs/*）→ emit `acp://permission` 或
  直接以错误响应拒绝 fs/*（Agent 文件操作走自有 fs，见 D2）。
- `acp_stop`：关 stdin 等退出（2s 后 kill）。进程死亡 → emit `acp://status`
  dead + 原因；前端会话标 error 态。
- 命令面：acp_start / acp_status / acp_session_new {cwd} / acp_session_load
  {acpSessionId, cwd} / acp_prompt {sessionId, text}（async，返回
  {stopReason, usage}）/ acp_cancel {sessionId} / acp_set_config {sessionId,
  configId, value} / acp_respond_permission {requestId, optionId|cancelled}。

### R2 前端会话链路
- `services/acpService.ts`：invoke + `listen('acp://update'|'acp://status'|
  'acp://permission')` 封装；isTauri 双通道（mock 复用现有 conversationService
  事件形状适配为 ACP update 形状）。
- `useConversation` 重写消费面：ACP update → store 变更（沿用 applyEvent 映射
  思路：message chunk 追加/工具调用累积替换/usage 更新）；发送前门禁：
  agent store 检测 installed&&acpOk，否则 message.warning + 跳 /settings，
  不产生消息。
- 停止：acp_cancel；按钮态沿用 generating。
- 模型/模式：会话创建后从 configOptions 填充 MessageComposer 的模型下拉
  （替换 workspace.MODELS 静态数组为会话级 options；workspace.selectedModel
  作为新建会话默认值经 set_config_option 应用）。
- 权限：confirm 模式在 ConversationView 顶部渲染审批卡（工具名/输入摘要/三
  按钮）；auto 直接回 allow 首选项；plan 按工具只读白名单（read/glob/grep/
  list 类 allow，其余 deny 首选项）；「本会话总是允许」记内存 map。

### R3 持久化（Q2）
- 索引 `~/.lightr-vibe/lightr-vibe/sessions-index.json`：{id, title, kind,
  projectId, cwd, acpSessionId, model, mode, createdAt, updatedAt}。
- 消息 `~/.lightr-vibe/lightr-vibe/sessions/<id>.json`：turn 结束与消息 status
  转 done 时落盘（流式期间内存缓冲，崩溃丢失当轮可接受，文档声明）。
- 启动恢复：索引 → session store（消息懒加载：打开标签时读）；继续会话优先
  `acp_session_load`，失败则新建 ACP 会话但保留本地历史（标题标「已重连」）。
- 标题：首轮用户消息截断 20 字作标题（替换 mock 种子标题逻辑）。

### R4 UI 适配
- MessageItem/ToolCallPanel 兼容真实工具调用形状（name/input/result/status）；
  thinking 块映射 ACP 的思考变体（若 update 含 thought 类 chunk）。
- usage 面板接真实 usage_update（token 数、contextWindow 取 model limit 或
  128k 兜底）。
- 会话错误态：ACP dead/凭据失败 → 对话区错误消息卡（含「去设置」按钮）。

### R5 回归与双通道
- 浏览器 dev：mock 通道（现有 mock 流）+ 提示条；权限卡/模型下拉有 mock 数据。
- 设置页、Agent 管理、子任务 1 文件功能不受影响。

## Acceptance Criteria

1. `pnpm build` + `cargo check` 通过。
2. Tauri + 已装 opencode：新建会话发送「列出当前目录文件」→ 对话区出现真实
   流式回复与工具调用（read/list 类）面板内容；停止按钮 1s 内中断（cancelled）。
3. confirm 模式触发写操作工具时出现审批卡；允许后工具继续、拒绝后 Agent 收到
   denied 并回复说明；auto 模式无卡直接执行；plan 模式写工具被拒。
4. 切换模型下拉 → set_config_option 生效（后续回复由所选模型产生，usage 更新）。
5. 重启应用：会话列表恢复；打开历史会话可见落盘消息；继续发送成功（load 或
   重连路径）。
6. 未安装/检测 FAIL：发送仅出引导提示，无假回复、无崩溃。
7. 无凭据环境：会话可建立（Zen 模型），切到需凭据模型时报错消息卡而非挂死。
8. 浏览器 dev：mock 全功能 + 提示条。
9. `~/.lightr-vibe/` 根既有文件未被触碰。

## Out of Scope

- 多会话并发 prompt（同会话串行；跨会话并发允许但 UI 不强调）。
- 会话分叉/fork、opencode 私有扩展（除 probe 记录的 capability 探测）。
- 终端类工具（terminal/*）client 实现 —— fs/* 与 terminal/* 一律拒绝并提示
  「由 Lightr-vibe 文件面板承担」。
- 消息全文检索、导出。
