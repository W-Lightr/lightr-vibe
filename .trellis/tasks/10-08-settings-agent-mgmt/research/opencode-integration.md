# Research: OpenCode 集成事实（设置页 / Agent 管理）

任务：10-08-settings-agent-mgmt。本文件为规划期证据存档，供 implement/check 子代理引用。

## 1. 本机 OpenCode 环境（2026-10-08 实测）

- 可执行：`C:\Users\29028\.version-fox\sdks\nodejs\opencode`（+ `.cmd` shim，version-fox 管理）
- `opencode --version` → `1.18.35`
- `opencode acp` 子命令存在（"start ACP (Agent Client Protocol) server"），
  支持 `--cwd`、`--port`、`--hostname` 等；**本期只探测不启动**。
- 注意：PATH 中是 `.cmd` shim → Rust 侧 `std::process::Command::new("opencode")`
  （CreateProcess）**不能**直接执行 `.cmd`；需回退 `cmd /C opencode ...`。
  检测顺序：直接 spawn → 失败则 `cmd /C` → 再失败判为未安装。

## 2. 配置文件位置与格式

- 路径：`~/.config/opencode/opencode.jsonc`（JSONC，可能含注释/尾逗号）
- 实测顶层键：`$schema`、`provider`
- provider 实测结构：

```jsonc
"GS": {
  "npm": "@ai-sdk/anthropic",
  "name": "GS",
  "options": { "baseURL": "https://...", "apiKey": "sk-..." },
  "models": {
    "Qwen3-Coder-Next-FP8": {
      "name": "Qwen3-Coder-Next-FP8",
      "limit": { "context": 230000, "output": 230000 },
      "attachment": false, "reasoning": true, "tool_call": true, "temperature": true
    }
  },
  "whitelist": ["Qwen3-Coder-Next-FP8"]
}
```

- **安全注意**：用户真实 apiKey 存于此文件。UI 默认掩码显示；本 research 不复制
  完整密钥值（上面以 `sk-...` 省略）。

## 3. 官方 schema（https://opencode.ai/config.json，2026-10-08 抓取）

- 顶层 `model`：string，`provider/model` 格式（如 `anthropic/claude-2`）
- 顶层 `small_model`：**snake_case**，string，同格式（用于标题生成等小任务）
- 顶层 `provider`：object（additionalProperties = ProviderConfig）
  - ProviderConfig：`npm`、`name`、`options{baseURL,apiKey,...}`、`models{...}`、
    `whitelist: string[]`、`env: string[]`（该 provider 需要的环境变量**名**列表）
- 顶层 `permission`：PermissionConfig = 单一 action 或对象：
  键 = `read, edit, glob, grep, list, bash, task, external_directory, todowrite,
  question, webfetch, websearch, lsp, doom_loop, skill`（+ additionalProperties）
  值 = PermissionRuleConfig = `ask|allow|deny` 或 对象映射（本期 UI 只写简单三态）
- 顶层**未确认**存在通用 `env` 对象（检索仅命中 provider 级 `env: string[]` 与
  MCP 的 `environment`）→ 环境变量不写全局配置，改存应用级（见 design.md D3）。

## 4. 仓库内相关现状

- 设置入口占位：`src/views/coding/components/TopBar.vue:117`
  （`notImplemented('设置')`）；HomeView 无入口。
- 路由：`src/router/index.ts`（`/`、`/coding`）；新增 `/settings` 顶层路由。
- Tauri capabilities（`src-tauri/capabilities/default.json`）：仅 sidecar 相关
  shell 权限 + 窗口权限。**本设计不新增任何 capability**（检测走 Rust
  std::process，文件读写走 Rust std::fs，均不需前端权限）。
- 应用数据目录：`src/utils/appData.ts` → `~/.lightr-vibe/`（浏览器回退占位）。
- 图标：`@lobehub/icons-static-svg/icons/` 含 `opencode.svg`（currentColor 内联
  SVG）、`claudecode-color.svg`、`codex-color.svg`、`cline.svg`、`cursor-text.svg`。
- 窗口控件/拖拽模式参考：`src/views/HomeView.vue`（data-drag-region + 自绘三键）。
- UI 规范：`.trellis/spec/frontend/ui-style.md`。

## 5. JSONC 安全写回方案

- 依赖 `jsonc-parser`（VS Code 同源，零依赖，含 `parse` 与 `modify(text, path,
  value, formattingOptions)` —— 返回最小编辑区间，**保留注释与其余格式**）。
- 流程：Rust 读原始文本 → 前端 jsonc-parser.modify 生成补丁文本 → Rust 写回
  （写前同目录 `.bak-<ts>` 备份，保留最近 5 份）→ 返回新文本确认。
- 冲突保护：读时记录 mtime；写前 Rust 比较 mtime，不一致返回
  `conflict` 错误，前端提示重新加载。
