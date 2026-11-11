# PRD — 升级 claude-agent-acp 0.104 并适配 ACP v2

## 背景

- `@agentclientprotocol/claude-agent-acp` 0.103.0 起只服务 ACP v2，移除了 v1 bridge。
- 本项目 `src-tauri/src/acp.rs` 当前固定 `protocolVersion: 1`，直接升级到 0.104.0 会让
  Claude Code 引擎在 `initialize` 阶段失败（`-32022 Unsupported protocol version`）。
- 用户要求升级到 0.104.0，同时保留 opencode / codex 等 v1 引擎的可用性。

## 目标

1. 让 `claude-agent-acp` 0.104.0 在本项目内完整可用（握手、会话、流式、权限、取消、历史恢复、委派、自动化）。
2. 保留 v1 协议路径：0.90–0.102、opencode、codex 不需要降级或改配置。
3. 前端尽量保持现有 ACP 事件/返回值契约，v2 差异在 Rust 边界归一化。
4. 升级动作可回滚、可诊断，不在应用内自动执行全局安装。

## 非目标

- 不实现 ACP v2 的全部新特性（terminal display、session/fork、session/delete、background work 等），只实现本项目现有功能所需的子集。
- 不改 opencode / codex 的启动与探测逻辑。
- 不在本任务内升级 opencode-ai 或安装 codex-acp。
- 不把前端整体重写为 v2 类型模型。

## 需求

### R1 协议协商

- 每个 ACP 连接在 `initialize` 时协商协议版本。
- 优先请求 v2；若 Agent 明确不支持（`-32022`，或 `-32602` 且错误指向 `protocolVersion`），回退 v1。
- 协商结果按连接保存，后续 `session/*`、prompt、权限、update 处理都按该连接的协议分支。
- 两版都失败时返回包含两次失败原因的可诊断错误，不能挂起。

### R2 v2 握手与能力

- v2 `initialize` 使用 `protocolVersion: 2`、`capabilities`、`info`。
- 客户端能力只声明 `elicitation.form`；不声明已移除的 `fs` / `terminal` / `session.notices`。
- 解析 v2 响应 `info` 作为 agent 信息；v1 继续解析 `agentInfo`。
- 记录并暴露实际协商到的 `protocolVersion`。

### R3 会话生命周期

- `session/new` 在 v2 下允许省略 `mcpServers`，但本应用继续传注入的 MCP 列表。
- v2 `session/load` 映射为 `session/resume`，参数包含 `sessionId`、绝对 `cwd`、完整
  `mcpServers`、`replayFrom: { "type": "start" }`。
- v2 恢复历史时，重放更新在响应前到达；现有 replay 抑制逻辑继续生效，不重复渲染。
- v2 `session/close` 先收敛最终更新和 idle 状态，再返回；关闭流程不悬挂。

### R4 Prompt 生命周期

- v2 `session/prompt` 的响应只表示消息已插入；turn 结束以该会话的 idle `state_update` 为准。
- `state_update` 的 `running` / `requires_action` 继续等待；`idle` 带 `stopReason` 时结束。
- `stopReason` 映射保持现有前端语义：`end_turn`、`cancelled`、`refusal`、`max_tokens`、
  `max_turn_requests`；idle `error` 转为可见失败。
- 空闲超时仍按「两次活动之间静默 ≥ 300s」判定；v2 prompt 的 ack 超时单独处理。
- `usage` 从 v2 idle `state_update` 提取，尽量保持现有 `PromptResult.usage` 形状。

### R5 更新与展示

- `tool_call_update` 首次出现时前端按新工具卡创建，后续按增量 patch；沿用现有前端能力。
- v2 `plan_update` 归一化为现有 `plan` 事件，`entries` 整体覆盖。
- v2 `config_option_update` 的 `configId` / `groupId` 归一化为现有 `id` / `group`。
- v2 整消息更新（`agent_message` / `agent_thought`）在 live 场景下尽量归一化为现有 chunk；
  无法无损表达时不得重复追加或破坏已有正文。
- `state_update`、terminal 类更新不进入 transcript。
- 未知 v2 变体安全忽略，不 panic、不阻断后续更新。

### R6 权限与提问

- v2 `session/request_permission` 的 `title` / `description` / `subject` 归一化为现有
  `acp://permission` 事件的 `toolCall`、`meta` 字段；`options` 原样保留。
- `subject.type == "tool_call"` 使用其中的 `toolCall`；`type == "command"` 合成可展示的
  toolCall（含 command/cwd）；无 subject 时用 title/description 合成。
- 权限答复、自动化自动放行、取消时回 `cancelled` 的行为保持。
- `elicitation/create` 保持现有形状与处理。

### R7 配置项

- v2 `session/set_config_option` 请求带 `type`：字符串值用 `"id"`，布尔值用 `"boolean"`。
- v2 响应与 `config_option_update` 中的 `configId` / `groupId` 归一化为前端现有字段。
- 模型、思考强度、模式等现有配置入口在 0.104.0 下可用。

### R8 认证与错误

- 识别 `authMethods` 非空的情况；若无法完成认证，返回可操作错误（提示运行登录命令或检查凭据），不能卡在生成中。
- 识别 `-32022`（版本不支持）、`-32800`（prompt 尚未插入/已取消）等 v2 错误码。
- 连接失败、初始化失败、协议解析失败都要通过现有 `acp://status` / 命令错误面暴露。

### R9 兼容与回滚

- 0.102.0 / 0.90.0、opencode、codex 继续走 v1；不得要求用户升级这些引擎。
- 提供环境开关或等价机制，在 v2 出现问题时强制回退 v1（`LIGHTR_ACP_PROTOCOL=v1`）。
- 升级/回滚命令写入文档；应用不自动执行 `npm i -g`。

### R10 验证

- Rust 单元测试覆盖：版本协商、initialize 参数/响应、session 方法选择、prompt 等待、
  update 归一化、permission 归一化、config 参数。
- `pnpm build`、`cargo test`、`cargo check` 通过。
- 真机矩阵：0.104.0（v2）与 0.102.0（v1 回退），覆盖普通会话、权限、计划、取消、
  历史恢复、委派、自动化。

## 验收标准

- **AC1**：安装 0.104.0 后，Claude Code 会话 `initialize` 成功，`protocolVersion` 为 2，
  agent 信息正确显示。
- **AC2**：切回 0.102.0 后，同一套代码通过协商回退 v1，Claude Code 会话正常。
- **AC3**：0.104.0 下新建会话、发送 prompt、接收正文/思考/工具更新，`end_turn` 正常结算。
- **AC4**：0.104.0 下权限卡能显示标题/目标，允许/拒绝均生效；自动化会话自动放行。
- **AC5**：0.104.0 下计划清单、配置项（模型/思考强度/模式）正常展示与切换。
- **AC6**：0.104.0 下停止生成返回 `cancelled`，无悬挂 prompt，pending 权限被取消。
- **AC7**：0.104.0 下重新打开历史会话，`session/resume` 重放不产生重复消息。
- **AC8**：仅支持 v1 的适配器不被 v2 握手阻断；两版都失败时错误信息包含版本与原因。
- **AC9**：opencode 与已安装的 codex-acp 无行为回归。
- **AC10**：委派系统提示与 `lightr-ask-mcp` 注入在 0.104.0 下仍生效。
- **AC11**：`pnpm build`、`cargo test`、`cargo check` 通过；无新增 clippy/TS 错误。
- **AC12**：升级/回滚命令与已知限制写入任务文档和 `tauri-bridge` 规格。

## 约束

- 不修改 `src-tauri/capabilities` 与窗口相关代码。
- 不把协议分支散落到组件；协议差异集中在 Rust ACP 层。
- 不引入新的前端依赖。
- 不执行 `npm i -g` 直到代码验证通过；全局安装需用户已确认。
- 保留现有 `connId` / `sessionId` 路由不变量与 per-session 进程模型。
