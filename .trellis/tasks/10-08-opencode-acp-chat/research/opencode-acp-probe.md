# opencode 1.18.35 `opencode acp` 实测报告

> 调研日期：2026-10-08，Windows 11，本机 opencode 1.18.35（version-fox 安装）。
> 实测方式：Python subprocess 直接驱动真实 exe，stdin/stdout 走 JSON-RPC 行协议，全部原文记录到 wire.log。
> 探针脚本快照存于本目录 `probes/`（probe.py=握手+prompt、cancel_probe.py=取消语义、err_probe.py/probe_c.py/probe_d.py=错误形态）。
> 临时工作目录 `%TEMP%\acp-probe-work`、`%TEMP%\acp-probe-fakehome` 已清理。

---

## 0. Windows spawn 注意事项（重要）

- PATH 上的 `opencode` 是 **version-fox 的 `.cmd` shim**：
  `C:\Users\29028\.version-fox\sdks\nodejs\opencode.cmd` → 内部执行
  `"%dp0%\node_modules\opencode-ai\bin\opencode.exe" %*`。
- `.cmd` 无法被 `CreateProcess` 直接执行（见 `.trellis/spec/frontend/tauri-bridge.md` Gotcha）。两条可行路线：
  1. **Rust 直接 spawn 真实 exe**（本次实测方式）：`node_modules\opencode-ai\bin\opencode.exe acp`，配 `CREATE_NO_WINDOW (0x0800_0000)`。exe 路径可由 `agent_detect_opencode` 的 `binPath` 探测结果推导（shim 内容固定指向 `node_modules\opencode-ai\bin\opencode.exe`）。
  2. 沿用 `agent.rs::run_probe` 的「直接 spawn → `cmd /C opencode` 回退」链。对长驻双向 stdio 进程，**路线 1 更干净**（`cmd /C` 中转多一层进程，kill 时可能杀不到真身；且 stdin/stdout 仍走管道，可行但进程树复杂）。
- `opencode acp` 支持的 flags：`--print-logs`（日志到 stderr）、`--log-level DEBUG|INFO|WARN|ERROR`、`--pure`（不加载外部插件）、`--cwd <dir>`、以及 `--port/--hostname/--mdns/--cors`（HTTP 服务形态存在，但 stdio 模式开箱即用，无需传参）。
- **默认 stderr 静默**：不加 `--print-logs` 时 stderr 全程 0 行输出（实测确认），协议流不受污染。
- wire 是 UTF-8。实测控制台出现中文 mojibake 是 Windows console 代码页问题，**不是协议问题**；Rust 侧按 UTF-8 字节流解码即可。

## 1. initialize 实测（真实响应原文）

请求（client 侧最小能力集）：

```json
{"jsonrpc":"2.0","id":0,"method":"initialize","params":{
  "protocolVersion":1,
  "clientCapabilities":{"fs":{"readTextFile":false,"writeTextFile":false}},
  "clientInfo":{"name":"acp-probe","version":"0.1.0"}}}
```

响应（~1.2s 后到达）：

```json
{"protocolVersion":1,
 "agentCapabilities":{
   "loadSession":true,
   "mcpCapabilities":{"http":true,"sse":true},
   "promptCapabilities":{"embeddedContext":true,"image":true},
   "sessionCapabilities":{"close":{},"fork":{},"list":{},"resume":{}}},
 "authMethods":[{"id":"opencode-login","name":"Login with opencode","description":"Run `opencode auth login` in the terminal"}],
 "agentInfo":{"name":"OpenCode","version":"1.18.35"}}
```

结论：
- protocolVersion 1 协商成功（int，不是字符串）。
- `loadSession` + `sessionCapabilities.list/resume/close` 全支持 → 会话持久化/历史列表/恢复都可用。
- **`fork`** 是 opencode 私有扩展 capability（规范 sessionCapabilities 无此字段）——忽略即可，`_meta` 之外出现未知字段要容忍。
- promptCapabilities：image + embeddedContext（无 audio）→ prompt 可带图片和 resource 内嵌上下文。
- `authMethods` 恒有一条 `opencode-login`（terminal 引导型：**让用户在终端跑 `opencode auth login`**），即使本机已登录也返回。它不是 OAuth-in-band 流程，Rust 侧不需要实现 authenticate 交互，只需在鉴权失败时展示引导文案。

## 2. session/new 实测

请求：`{"cwd":"C:\\Users\\29028\\AppData\\Local\\Temp\\acp-probe-work","mcpServers":[]}`

响应（**耗时 7.6~8.9s**——首次要拉取/枚举模型列表，务必给足超时 ≥30s）：

```json
{"sessionId":"ses_ee439f479ffeUZRmG940dgGz1A",
 "configOptions":[
  {"id":"model","name":"Model","category":"model","type":"select",
   "currentValue":"GS/Qwen3-Coder-Next-FP8",
   "options":[{"value":"opencode/big-pickle","name":"OpenCode Zen/Big Pickle"}, "...共20个"]},
  {"id":"mode","name":"Session Mode","category":"mode","type":"select",
   "currentValue":"build",
   "options":[
     {"value":"build","name":"build","description":"The default agent. Executes tools based on configured permissions."},
     {"value":"plan","name":"plan","description":"Plan mode. Disallows all edit tools."}]}]}
```

结论：
- 响应无 `modes` 字段（null）——opencode 已全面走 **configOptions 新机制**。前端模型/模式选择器直接渲染 configOptions（select 型）。
- `currentValue.model` 来自用户 `~/.config/opencode/opencode.jsonc` 的顶层 `"model": "GS/Qwen3-Coder-Next-FP8"`；options 列表 = opencode 内置 Zen 免费模型 + **用户 config 里 provider.models 的全部组合**（`provider键/model键` 格式，如 `GS/Qwen3-Coder-Next-FP8`、`火山引擎/deepseek-v4-flash`）。
- configOptions 集合是**动态的**：切到带 reasoning 的模型后会新增 `effort` 选项（实测 `{'model': 'opencode/exo-free', 'effort': 'high', 'mode': 'build'}`）；无凭据干净环境下只有 `model`+`mode`。前端必须按响应重建选项集，不能写死。
- 模型 id 含非 ASCII（如 `火山引擎/...`）→ Rust/TS 全链路保证 UTF-8。

## 3. session/prompt 流式实测（真实模型，短消息，完成即止）

请求 prompt `[{"type":"text","text":"Reply with exactly the two characters: OK"}]` 后事件序列（时间轴自 wire.log）：

```
+0.00s  SEND session/prompt
+0.00s  UPDATE available_commands_update   ← 项目 skills 作为 slash commands（belfry、brainstorming…）
+2.7s   UPDATE agent_message_chunk {messageId:"msg_11bc…", content:{type:"text",text:"OK"}}
+2.7s   UPDATE usage_update {used:13262, size:230000, cost:{amount:0,currency:"USD"}}
+2.7s   RECV id=2 {"stopReason":"end_turn","usage":{"inputTokens":13262,"outputTokens":2,"totalTokens":13264},"_meta":{}}
```

结论：
- `session/prompt` 是长请求，期间 server 推 `session/update`，最后才回 `stopReason` —— 与规范一致。
- PromptResponse 带 opencode 扩展字段 **`usage`**（inputTokens/outputTokens/totalTokens，reasoning 模型下还有 thoughtTokens/cachedReadTokens）→ 反序列化要容忍未知字段（serde `deny_unknown_fields` 禁用）。
- `available_commands_update` 在 turn 开始时就推（本项目 `.claude/skills` 被 opencode 识别为 slash commands），前端可用来填充 `/` 命令面板。
- 首 token 延迟 ~2.7s（走用户自建 GS 代理）。

### 工具调用形态（cancel 探针中捕获的 bash 工具流）

```
tool_call        {toolCallId:"call_e10c…", title:"bash", kind:"execute", status:"pending",
                  rawInput:{cwd:"…"}, locations:[{path:"…"}]}
tool_call_update {status:"in_progress"}                       ← 多次
tool_call_update {status:"in_progress", content:[{type:"content",content:{type:"text",text:"1\n2\n…"}}]}  ← 增量输出整体替换
tool_call_update {status:"completed", content:[…全量]}
```

- **未触发 `session/request_permission`**：本机 opencode 配置默认放行 bash（permission 配置宽松）。Rust 侧仍必须实现该方法（换环境/换配置就会弹）。
- tool_call_update 的 `content` 是**累积替换**语义（每次带全量文本），前端直接覆盖渲染，不要 append。

## 4. 取消语义实测（cancel_probe.py）

- 长输出 prompt 进行中（工具已 completed、LLM 还在生成）→ 发 `session/cancel` notification：
  - **+44ms** 即回 `{"stopReason":"cancelled","usage":{…全 0}}`（id=2 的 prompt response）。取消非常快。
- prompt 已结束后再发 `session/cancel`（idle 状态）：**安全 no-op**，无错误、进程不退出。
- 进程在 stdin 关闭后 **rc=0 干净退出**（多次实测，<1s）。`session/cancel` 后不需要杀进程。

## 5. 错误形态实测（err_probe.py / probe_d.py）

| 场景 | 真实响应 |
|---|---|
| prompt 用不存在的 sessionId | `error -32602 "Invalid params: session not found: ses_bogus"` data:{sessionId} |
| 未知方法 `session/nonexistent` | `error -32601 "Method not found"` data:{method} |
| session/new 缺 cwd | `error -32602 "Invalid params"` data 为 zod 风格 `{cwd:{_errors:[…]}}` |
| set_model 指定未配置 provider | `error -32602 "model not found: anthropic/claude-sonnet-4-5"` data:{providerId,modelId} |
| authenticate 用假 methodId | `error -32602 "unknown auth method: bogus-login"` |
| stdin 写入非 JSON 行 `gibberish` | **进程不崩溃、无响应**（静默忽略），继续正常服务直到 stdin 关闭后 rc=0 |

### 无凭据环境（isolated fake HOME，无任何 auth.json/配置）

- `initialize` / `session/new` **不报错**：authMethods 照常返回，模型列表只剩 11 个 `opencode/*` Zen 模型（无用户 provider）。
- `session/prompt`（默认模型 `opencode/big-pickle`，Zen 免费通道）**直接成功** `stopReason:"end_turn"`——Zen 免费模型无需凭据。
- 推论：**「无凭据失败」不会在握手阶段暴露**，而是（a）切到需凭据的模型后 prompt 报错，或（b）`set_model` 阶段就因模型不在列表而 -32602。Rust 侧对 prompt 的 error/`stopReason:"refusal"`/异常文本都要有 UI 兜底；引导文案 = `opencode auth login`（authMethods[0].description 原文）。

## 6. 模型切换 / 会话 API 实测

- **`session/set_model`（opencode 扩展方法，规范无）**：`{sessionId, modelId:"opencode/exo-free"}` → `result {}`，随后 configOptions 的 model.currentValue 确认已变。**可用**。
- **`session/set_config_option`（规范方法）**：`{sessionId, configId:"mode", value:"plan"}` → 返回全量 configOptions（currentValue 已更新）。**可用，优先用它**（set_model 是私有扩展）。切模型：`configId:"model", value:"GS/Qwen3-Coder-Next-FP8"`。
- `session/set_mode`（规范旧方法）也返回 `{}` 成功，但别依赖。
- `session/load`（同进程重载刚建的会话）：成功，响应携带 configOptions；重放历史应在响应前以 session/update 推送（本次会话无历史消息，未见重放事件——多轮会话后的 load 重放行为留待实现阶段验证）。
- `session/list` capability 已声明，未实测具体返回（实现会话历史页时验证）。

## 7. 进程生命周期结论

| 行为 | 实测 |
|---|---|
| 启动到 initialize 响应 | ~1.2s（exe 直启） |
| session/new 首次 | **7.6~8.9s**（模型枚举/网络），后续同进程再建会话未测但预计更快 |
| stdin close | **<1s 内 rc=0 退出**，无需 kill |
| stderr | 默认全静默；`--print-logs --log-level INFO` 时输出结构化日志行（timestamp=… level=… message=…），包括 config 加载路径，可用于诊断 |
| idle 容忍 | 会话建好后长时间 idle 无心跳要求（本次探针间隔数十秒无异常） |

## 8. Rust 侧集成形态建议

### 进程模型：**每 Tauri app 一个长驻 `opencode acp` 子进程，多会话复用**

- 依据：session/new 首次 7-9s 很贵；进程本身支持多 session（sessionId 路由）；stdin 关闭即干净退出，shutdown 简单。
- 项目（cwd）切换不需要重启进程：cwd 是 session/new 的参数，不是进程的（`--cwd` 可省略）。
- 崩溃恢复：stdout EOF / wait() 返回 → 标记所有活动会话 broken，前端提示后可一键重启（重新 initialize + session/load 恢复历史，loadSession=true 支持）。

### 线程/事件管道

```
Command::spawn(exe, ["acp"])  // CREATE_NO_WINDOW；exe 路径来自 detect 的 binPath 推导，回退 cmd /C
  ├─ stdin  : 单写者（mpsc → writer 线程），所有请求/响应/通知序列化为一行 JSON + \n
  ├─ stdout : BufReader::lines 循环 → serde_json 解析 →
  │     response(id)        → oneshot 唤醒挂起的 tauri command future
  │     session/update      → app.emit("acp://update/{connId}", payload)  ← 直接转发原始 update JSON
  │     agent→client request → permission: emit 事件等前端回 invoke；fs/*: 未声明 capability 不会收到
  └─ stderr : 独立线程 drain（防管道满死锁，spec §4），ring buffer 留存最后 N 行供诊断
```

- **前端契约建议**：Tauri event 名 `acp://update`（payload 含 sessionId），事件体就是 `params.update` 原样透传（sessionUpdate 判别字段 + 字段），TS 侧按 acp-protocol.md §4 的 union 建模。前端已有的 conversationService mock 事件流可 1:1 映射：agent_message_chunk→消息追加、tool_call/tool_call_update→工具卡片、plan→计划面板、usage_update→token 角标、available_commands_update→slash 命令面板。
- 命令面（与 agent.rs 现有风格一致，camelCase serde）：
  - `agent_chat_start(cwd) -> { connId }`（幂等：已有活进程直接复用）
  - `agent_session_new(cwd) -> { sessionId, configOptions }`
  - `agent_session_prompt(sessionId, prompt) -> { stopReason }`（长 future，await 到 turn 结束；流式内容走事件）
  - `agent_session_cancel(sessionId)`（fire-and-forget notification；同时把本地 pending permission 自动回 cancelled）
  - `agent_session_set_config(sessionId, configId, value) -> { configOptions }`
  - `agent_permission_respond(requestId, optionId | "cancelled")`
  - `agent_session_list()` / `agent_session_load(sessionId, cwd)`（历史会话）
  - 所有 struct `#[serde(rename_all="camelCase")]`；错误统一 `Result<_, String>`，错误串保留 JSON-RPC code（如 `"acp:-32602 session not found"`）供前端分支。
- 超时基线：initialize 15s、session/new **30s**、set_config 10s、prompt 不设硬超时（靠 cancel）。
- kill 兜底：正常退出靠 stdin close（实测 rc=0）；超时场景 `child.kill()` + 等 wait 收尸。

## 9. 未验证项（实现阶段确认）

1. `session/load` 对**有多轮历史**的会话的重放事件完整性（本次仅空会话 load）。
2. `session/list` 的返回形状与跨进程持久性（opencode 会话存储在 `~/.local/share/opencode`）。
3. 触发 `session/request_permission` 的真实场景（需 permission 配置收紧的 cwd）。
4. 需凭据模型在凭据失效时 prompt 的具体错误形态（本次 fake-home 用免费 Zen 模型绕过了）。
5. `--pure` 模式（不加载插件/skills）对启动速度与 available_commands 的影响——若想跳过 skills 扫描加速 session/new 可试。
