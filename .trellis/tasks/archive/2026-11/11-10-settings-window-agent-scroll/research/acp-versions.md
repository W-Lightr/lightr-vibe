# ACP 适配器版本核查（2026-11-10）

## 结论先行

| 包 | npm latest | 本机已装 | 建议 |
|---|---|---|---|
| `@agentclientprotocol/claude-agent-acp` | **0.104.0** | **0.90.0** | **不要直接升到 0.104.0**。0.103.0 起 BREAKING：只服务 ACP v2 并移除 v1 桥，本项目客户端固定 `protocolVersion: 1`。若必须升级，最高到 **0.102.0**（最后一个 v1 兼容版本）；否则维持现状。 |
| `@agentclientprotocol/codex-acp` | **2.6.0** | 未安装 | 可安装，但必须先做真机冒烟（initialize / session/new / prompt / 权限卡）。v1 是否完整保留未在本仓库验证。 |
| `opencode-ai` | **1.19.5** | **1.19.0** | 可升（补丁版本）。升后跑一次真实会话冒烟。 |

本仓库 `package.json` / `pnpm-lock.yaml` / `Cargo.toml` 均不依赖上述包；它们是用户全局安装的
外部 CLI，App 只负责探测与拉起。因此「更新」= 改用户全局安装，不是改项目 lockfile。

## 数据来源

- `npm view <pkg> version / dist-tags / time.modified`（2026-11-10 实测）
- `https://registry.npmjs.org/<pkg>/<version>`（engines / bin）
- GitHub Releases：`agentclientprotocol/claude-agent-acp`、`agentclientprotocol/codex-acp`
- 本机：`npm ls -g --depth=0`、`claude-agent-acp --version`、`opencode --version`、`node -v`

实测环境：Node `v24.16.0`、npm `11.13.0`。两个适配器均要求 `node >= 22`，满足。

## claude-agent-acp：为什么不能直接升

### 版本时间线（GitHub Releases）

- `0.90.0` 2026-10-13 ← 本机已装
- `0.100.0` 2026-11-02 — 按 ACP 最新规范校验 session/prompt/config/auth；对齐 v2 的
  `session/cancel`、`session/close`
- `0.101.0` 2026-11-05 — GitHub release 增加 bun 单文件可执行产物（npm `bin` 仍是
  `dist/index.js` shim，不影响现有解析逻辑）
- `0.102.0` 2026-11-09 — 背景任务、checklist plan、SDK 2.5.0
- **`0.103.0` 2026-11-10 — BREAKING：`serve ACP v2 with a direct v2 agent and remove the v1 bridge`（#1560）**
- `0.104.0` 2026-11-10 — ACP auth/status RFD、远端 Anthropic Console 登录

### 与本项目的冲突点

`src-tauri/src/acp.rs` 的 `initialize` 固定发送：

```json
{ "protocolVersion": 1, "clientCapabilities": {...}, "clientInfo": {...} }
```

前端 `src/types/acp.ts` 也只按 v1 的形状消费 `session/update`。0.103.0 移除 v1 桥后，
客户端要么按 v2 握手/消息形状通信，要么握手失败。当前代码只记录返回的
`protocolVersion`，没有 v2 分支，因此升级会带来真实回归风险。

### 推荐动作

- 保持 `0.90.0`：零风险，但错过 0.91–0.102 的修复。
- 或升级到 `0.102.0`（最后一个带 v1 桥的版本）：

```powershell
npm i -g @agentclientprotocol/claude-agent-acp@0.102.0
```

- 要使用 `0.104.0`：先单独立项实现 ACP v2 客户端支持（握手协商 + update/permission/elicitation
  形状），再升级；不要和本任务混做。

## codex-acp：未安装，升级前先冒烟

- latest `2.6.0`（2026-11-08），preview `2.6.1-preview.2`。
- 时间线：`2.2.2`（2026-10-10，本仓库 spec 实测版本）→ `2.3.0`（重新加入 ACP v2 支持）
  → `2.4.0` / `2.5.0` / `2.6.0`（SDK 2.2→2.4、subagent、elicitation、权限模式等）。
- 本仓库代码注释按 `codex-acp 1.1.8–1.6.2` / `≥1.7.0` 的权限 `_meta` 形态做兼容；
  2.x 未在本仓库真机验证。
- 建议：先安装 latest，跑 `initialize` → `session/new` → 一次 prompt → 权限卡/提问卡；
  出现协议形状不兼容时回退 `2.2.2`（spec 记录过的已验证版本）。

```powershell
npm i -g @agentclientprotocol/codex-acp@latest
```

## opencode-ai：补丁升级，可直接升

- latest `1.19.5`，本机 `1.19.0`；同一 minor 线的补丁版本。
- 注意：opencode 1.x 仍不发送 ACP `elicitation/create`，本仓库通过注入
  `lightr-ask-mcp` 补提问能力；升级后确认该注入链路仍正常。

```powershell
npm i -g opencode-ai@1.19.5
```

## 回归冒烟清单（任何升级后执行）

1. 设置 → Agent 管理：三个引擎徽章显示 PASS / 未安装符合预期，版本号正确。
2. 新建 Claude Code 会话：`initialize` 成功（无 `timeout` / protocol 错误），能收到首轮正文。
3. 触发一次工具权限：权限卡正常出现，选项 `kinds` 匹配正确（不是按数组位置）。
4. 触发一次 AI 提问（`lightr-ask-mcp`）：提问卡出现并能回传。
5. `session/load` 恢复历史：不重复渲染（replay 抑制仍生效）。
6. 长任务中途停止：`stop()` 收敛，无悬挂 prompt。

## 备注

- `@agentclientprotocol/sdk` 是适配器自己的依赖，不是本仓库依赖；本仓库没有 ACP SDK
  版本需要升级。
- `codex-acp` 依赖 `@openai/codex` 的平台可选依赖；若出现 `Missing optional dependency`
  或 `spawn EFTYPE`，按 spec 建议执行 `npm i -g @openai/codex@latest`（当前已装 0.153.4）。
