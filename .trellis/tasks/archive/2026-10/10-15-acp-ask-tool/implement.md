# Implement: 注入自有 ask 工具（任意引擎可提问）

需求 `prd.md`，设计 `design.md`。执行顺序：Rust（伴生 bin → 主进程桥接 → 注入）→ 前端（无改动，仅核对）→ 检查。

## 验证命令

```powershell
cargo check            # workdir: src-tauri
cargo test --lib       # workdir: src-tauri
pnpm vue-tsc --noEmit
pnpm build
```

---

## Step 0 · 前置实测（阻塞字段定稿）

- [ ] 抓 opencode 1.x `session/new` 对 `mcpServers` stdio 项的**字段形状**（`type`/`name`/
      `command`/`args`/`env`）——按 opencode 文档 §"MCP servers" 示例实现。
- [ ] 确认 bin 定位方式：dev 下 target 目录、生产下同 `current_exe` 目录（Tauri sidecar？）。

**产出**：回填 `design.md` §4 字段与 §3 端口/参数。

## Step 1 · Rust：伴生 bin `lightr-ask-mcp`

- [ ] 新 `src-tauri/src/bin/lightr_ask_mcp.rs`（`[[bin]]` 或默认 bin target）。
- [ ] stdio MCP：`initialize` / `tools/list`（暴露 `ask_user_question`）/ `tools/call` / `notifications/cancelled`。
- [ ] `ask_user_question` inputSchema：`question` + `options[]` + `multiSelect`（本期最小）。
- [ ] `tools/call` → 连 socket（`--socket`/`--token`/`--conn-id`）→ 发请求 → 阻塞等应答 → 返回结果。
- [ ] stdin 关闭 / 取消 → 终止挂起，不 hang。

**验证**：`cargo build` 产出 bin；手动 `echo initialize` 冒烟（可选）。
**回滚点**：新文件。

## Step 2 · Rust：主进程 socket 监听 + 桥接

- [ ] 回环 TCP 监听（`127.0.0.1:0`）拿端口；每会话随机 token。
- [ ] 处理 ask：`{connId,token,questions}` → 复用 `pending_question` + emit `acp://question`。
- [ ] 扩展 `pending_question` 条目为 `QuestionReply { Acp(rpcId), Mcp(oneshot<Answer>) }`，
      答复时按来源回写（JSON-RPC vs socket）。
- [ ] 取消/关闭：挂起 ask → decline（socket 断开/回 decline），不 hang。

**验证**：`cargo check` + `cargo test --lib`（桥接层单测：入→答→回传，用内存 oneshot）。
**回滚点**：`acp.rs` + 新模块。

## Step 3 · Rust：注入 `session/new.mcpServers`

- [ ] `acp_session_new` / `acp_session_load`（`acp.rs:1355,1377`）：构造 stdio MCP 项
      （command=lightr-ask-mcp 路径，args=--socket/--token/--conn-id）。
- [ ] 仅对支持 stdio MCP 的引擎注入（判定集中在注入处；opencode 支持）。
- [ ] 探针 `1035`：**不注入**（或注入即回收）；实现定稿。
- [ ] `--parent-pid` 兜底（伴生会话死自退）。

**验证**：`cargo check`。
**回滚点**：`acp.rs`。

## Step 4 · 前端核对（零改动）

- [ ] 确认 `acp://question` 事件形状与 `AskQuestionCard`/`useAcpQuestion` 无需改。

**验证**：`pnpm vue-tsc --noEmit` + `pnpm build`。
**回滚点**：无。

## Step 5 · 全量质量检查（trellis-check）

- [ ] 四门禁全绿。
- [ ] PAC1：`rg -n "codex|Codex|claude|Claude" src-tauri/src/acp.rs` 无引擎名分支（注入判定
      集中处除外）。
- [ ] PAC4：opencode 不调 ask 时零回归。
- [ ] 两条提问来源（ACP elicitation / MCP ask）都进同一卡、都能答。

## Step 6 · 实机验收（Tauri，人工）

- [ ] opencode 1.x 会话：让模型"用你的提问工具问我一个问题" → 弹**我们的卡** → 作答 → 继续。
- [ ] codex（若装）：原生 elicitation 路径仍工作。
- [ ] 取消/关闭会话：不 hang、无僵尸进程。

---

## 关键契约

- **注入只在 spawn/声明处**，协议路由不含引擎名（PAC1）。
- **复用既有提问卡链路**（前端零改动）。
- **回环 + token**：仅本机、可校验。
- **不 hang**：任何取消/断连路径伴生与主进程都必须回到底。

## Open Questions（Step 0 定稿）

- opencode `mcpServers` stdio 字段形状。
- bin 定位/打包。
- 探针是否注入。
