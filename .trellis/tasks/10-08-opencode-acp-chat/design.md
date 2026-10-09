# Design: OpenCode ACP 真实对话（子任务 2）

需求见 `prd.md`。协议契约见 `research/acp-protocol.md` / `opencode-acp-probe.md`。
桥接规范：`.trellis/spec/frontend/tauri-bridge.md`。

## 1. 架构

```
src-tauri/src/acp.rs        新：进程管理 + JSON-RPC 路由 + 事件 emit
src-tauri/src/lib.rs        + mod acp + 7 命令注册
src/services/acpService.ts  新：invoke/listen 封装 + 超时/状态机
src/services/acpMock.ts     新：浏览器 mock（现有 mock 事件流适配为 ACP update）
src/composables/useConversation.ts  重写消费面（ACP update → store）
src/composables/useAcpPermission.ts 新：权限请求队列 + 模式策略
src/stores/session.ts       + 持久化 hooks（索引/消息落盘/懒加载）
src/stores/workspace.ts     MODELS 静态数组降级为兜底；会话级 configOptions 优先
src/views/coding/components/
  MessageComposer.vue       模型下拉改会话级 options
  ConversationView.vue      审批卡挂载点
  PermissionCard.vue        新：审批卡组件
  MessageItem/ToolCallPanel 兼容真实形状
~/.lightr-vibe/lightr-vibe/
  sessions-index.json       会话索引
  sessions/<id>.json        消息落盘
```

## 2. Rust ACP 核心设计

- **进程**：`AcpProcess { child, stdin: Mutex<BufWriter>, next_id: AtomicU64,
  pending: Mutex<HashMap<u64, oneshot::Sender<Value>>> }`，全局
  `OnceLock<Mutex<Option<AcpProcess>>>`。spawn：解析真身（which opencode →
  读 .cmd 文本提取 exe 路径 → 失败 `cmd /C`）+ CREATE_NO_WINDOW + cwd=应用
  数据目录（ACP 自身 cwd 无关会话 cwd，session/new 传 cwd）。
- **读线程**：逐行 stdout → serde_json 解析（失败行跳过，probe 证实安全）→
  有 id 且无 method = response → 唤醒 pending；有 method 无 id = notification
  → `session/update` 等 → `app.emit("acp://update", payload{sessionId,update})`；
  有 method 有 id = client request → `request_permission` 转存 pending_client
  map 并 emit `acp://permission`；`fs/*`/`terminal/*` 立即以 JSON-RPC error
  (-32601 unsupported) 响应（D2）。
- **请求/超时**：`rpc_call(method, params, timeout)` 在 spawn_blocking 内等
  oneshot；超时移除 pending 并返回 Err("timeout")。initialize 10s /
  session/new 30s / prompt 300s / 其余 30s。
- **命令**（camelCase 返回）：acp_start→{status,protocolVersion,agentInfo}；
  acp_status；acp_session_new{cwd}→{sessionId,configOptions}；
  acp_session_load{acpSessionId,cwd}；acp_prompt{sessionId,text}→
  {stopReason,usage}（async spawn_blocking）；acp_cancel{sessionId}
  （notification，无响应）；acp_set_config{sessionId,configId,value}；
  acp_respond_permission{requestId,optionId?}（optionId=null → 回 cancelled
  语义选项按规范：返回 error -32800？probe 规范：client 对 pending permission
  回 cancelled outcome）。
- **死亡**：读线程 EOF → emit `acp://status {status:'dead',reason}` → 清全局；
  下次 acp_start 重建。

## 3. 前端数据流

- acpService 维护 `listen` 三订阅（模块单例，main 后初始化）；update 分发到
  useConversation 注册的 handler（按 sessionId 路由）。
- update → store 映射（沿用 applyEvent 思路重写）：
  agent_message_chunk{messageId,text} → 无则 pushMessage(streaming) 有则
  appendContent；tool_call/tool_call_update → **累积替换**写 toolCalls 条目；
  usage_update → addUsage；thought 类 → thinking 消息；
  plan/available_commands → 忽略（面板不展示）。
- prompt 完成（response 回）→ 消息 status done + 落盘。
- 发送门禁：agent store detect（复用设置页结果，缓存于 store）installed&&acpOk
  否则 message.warning('未检测到可用 Agent')+ router.push('/settings')。
- 权限策略（useAcpPermission）：queue ref；auto→立即 respond 首 allow 选项；
  plan→工具名匹配只读白名单（read/glob/grep/list/search/webfetch）allow 否则
  deny 首选项；confirm→入队渲染 PermissionCard（工具名/input 摘要/三按钮：
  允许一次/本会话总是允许/拒绝；总是允许记 sessionId→toolName map 命中即 auto）。
- 模型：session/new 的 configOptions.model.options → MessageComposer 下拉
  （label=name, value=value）；切换 → acp_set_config(configId='model')；
  mode 同理（build/plan 与 workspace.executionMode 不联动，独立下拉？→ D3：
  本期 mode 仅展示不暴露 UI，executionMode 继续驱动权限策略）。

## 4. 持久化

- 索引写时机：createSession / 标题确定 / updatedAt 变化（debounce 1s）。
- 消息落盘：turn 结束（prompt response）整会话 messages 序列化写
  sessions/<id>.json（简单全量覆写，体积可控；流式期间不写）。
- 恢复：启动读索引（不含 messages）；打开标签 → 若 store 无 messages 则读
  文件；继续发送：acp_session_load 成功沿用 acpSessionId，失败 acp_session_new
  + 本地历史保留 + 标题后缀「（已重连）」。

## 5. 决策

- **D1 长驻单进程多会话**：probe 证实 session 复用同一 server；避免每会话
  8s 握手。进程死 → 全部会话标 error + 一键重启（acp_start 幂等）。
- **D2 拒绝 client fs/terminal**：Agent 的文件操作改由 Lightr-vibe 自有 fs
  面板观察；避免 Agent 绕过权限模型写盘（安全边界）。
- **D3 mode 不暴露 UI**：executionMode（权限策略）与 ACP mode（Agent 行为）
  语义不同，混用会误导；本期 ACP mode 恒 build。
- **D4 流式期间不落盘**：崩溃丢当轮可接受（文档声明），换取写放大为零。
- **D5 mock 适配层**：acpMock 将现有 MockEvent 流翻译为 ACP update 形状，
  使 useConversation 只有一套消费逻辑（双通道在 service 层收敛）。

## 6. 兼容/回滚

- conversationService 保留为 acpMock 的数据源（不删），回滚 = useConversation
  指回旧消费面（git 单文件）。
- workspace.MODELS 保留为「无 configOptions 时」兜底。
- 不动设置页/Agent 管理/子任务 1 文件面。

## 7. 测试要点

- JSON-RPC 路由单测式脚本（probes/ 已有）：握手/new/prompt/cancel 往返。
- 超时矩阵：session/new 30s 边界（mock 延迟注入）。
- 权限三模式各一用例 + 「总是允许」命中用例。
- 持久化往返：发消息→杀进程→重启→历史可见→继续发送。
- 进程死亡注入（taskkill）→ status dead → 会话 error 卡 → 重启恢复。
