# 设置界面与 Agent 管理（OpenCode 集成）

## Goal / 用户价值

为 Lightr-vibe 增加「偏好设置」页：左侧设置菜单（首个条目为 Agent 管理），
Agent 管理页统一管理编码 Agent 的接入状态、版本/诊断预检、模型与 Provider 配置、
权限与环境变量。首个支持 OpenCode（ACP 能力），配置读写与检测全部真实落地
（Tauri 环境），浏览器 dev 环境回退 mock，不伪造功能。

参考设计图：浅色三列布局（设置菜单 / Agent 列表 / 详情），Chrome 风格卡片与
状态徽章（PASS/FAIL），遵循 `.trellis/spec/frontend/ui-style.md`。

## Confirmed facts（仓库/环境证据）

- 入口现状：`src/views/coding/components/TopBar.vue:117` 已有齿轮按钮，
  当前 `notImplemented('设置')` 占位；`HomeView.vue` 无设置入口。
- 路由现状：`src/router/index.ts` 仅 `/`（Home）与 `/coding`（WorkspaceLayout 嵌套）。
- Tauri 能力现状：`src-tauri/capabilities/default.json` 仅允许 sidecar
  `binaries/agent-sidecar` 的 spawn/execute/kill/stdin；**无 fs 插件、无系统
  命令白名单**。`tauri.conf.json` bundle.externalBin 已声明 agent-sidecar。
- 本机 OpenCode：已安装（version-fox 管理的 nodejs shim，`opencode --version`
  → `1.18.35`）；存在 `opencode acp` 子命令（ACP server）。
- OpenCode 配置文件：`~/.config/opencode/opencode.jsonc`（JSONC，含注释风险），
  实测结构：`{ $schema, provider: { <name>: { npm, name, options: { baseURL,
  apiKey }, models: { <id>: { name, limit: {context,output}, attachment,
  reasoning, tool_call, temperature, modalities } }, whitelist?: string[] } } }`。
- OpenCode 官方配置 schema（opencode.ai/config.json，已索引）：顶层含
  `model` / `smallModel`（`provider/model-id` 字符串）、`permission`
  （read/edit/glob/grep/list/bash/... → `ask|allow|deny` 或按对象细化）、
  `provider`、`env` 等；`PermissionActionConfig` 枚举 = ask/allow/deny。
- 图标资源：`@lobehub/icons-static-svg/icons/` 含 `opencode.svg`、
  `claudecode-color.svg`、`codex-color.svg`、`cline.svg`、`cursor-text.svg` 等
  （`currentColor`、1em 内联 SVG），可直接作 Agent 列表图标。
- 应用数据目录约定：`~/.lightr-vibe/`（`src/utils/appData.ts`，浏览器回退占位路径）。
- UI 规范：`.trellis/spec/frontend/ui-style.md`（白底 neutral 灰阶、blue-500 强调、
  rounded-md、text-xs/[13px]、无阴影、中文文案）。

## Requirements

### R1 设置页骨架
- 新增**独立全屏路由 `/settings`**（用户已确认），不嵌套于 WorkspaceLayout；
  左侧设置菜单 + 右侧内容区。本期菜单仅「Agent 管理」真实可用；其余设计图
  条目（外观/常规/协作/MCP/Skills/技能包/模型供应商/快捷消息/浏览器操作/
  电脑操作/快捷键/版本控制/消息渠道/Web 服务/运行日志/系统）以禁用态呈现，
  点击给 `message.info('「X」即将推出')` 占位反馈（与 TopBar `notImplemented`
  惯例一致）。
- 入口：TopBar 齿轮按钮跳转 `/settings`（替换 notImplemented）；HomeView
  右上角增加齿轮图标按钮。设置页左上提供返回按钮（`router.back()`，无历史
  时回 `/`）。
- 布局：左菜单（图标+文字，选中态 `bg-neutral-200/70`）+ 右内容区；顶部
  `data-drag-region` 拖拽条 + 自绘窗口控件（复用 HomeView 模式）。
- 菜单切换用页内 `activeSection` 状态（不建嵌套路由），本期仅一个真实分区。

### R2 Agent 管理页 — 列表
- 左列「Agent 列表」：OpenCode 为本期唯一可配置 Agent；其余设计图中的 Agent
  （Claude Code、Codex CLI、Gemini CLI、Cline、Cursor 等）以**禁用行**呈现
  （lobehub 图标 + 名称 + 「即将推出」徽章，不可选中）。
- 每行：拖拽把手图标（视觉占位，本期不可拖）、图标、名称、状态点、
  检测徽章（PASS 绿 / FAIL 红 / 检测中 spin / 未安装灰）+ 刷新按钮。
- 选中 OpenCode 后右列展示详情；列表状态来自真实检测（R3）。

### R3 OpenCode 详情 — 版本与诊断预检
- 详情头：图标 + 名称 + `binary` 徽章 + 描述 + 启用开关（本期开关仅记录到
  应用设置，不影响会话链路）。
- 「预检项：N」+「诊断」按钮：逐项执行并展示 PASS/FAIL 徽章，可折叠展开明细：
  1. 版本状态（`opencode --version`，解析 semver）
  2. Platform（OS/arch/PATH 解析到的可执行路径）
  3. 配置文件（`opencode.jsonc` 存在性与 JSONC 可解析性）
  4. ACP 能力（`opencode acp --help` 退出码/输出探测，仅探测不启动服务）
- 未安装/检测失败：详情区显示引导文案（安装命令提示），不显示配置编辑区。
- 浏览器 dev 环境：全部预检回退 mock（PASS + 假版本号），界面顶部显示
  「浏览器模式：数据为 mock」提示条。

### R4 OpenCode 详情 — 模型与 Provider 配置
- 主模型 / 小模型两个下拉（`provider/model-id`），选项来自已连接 Provider 的
  models；选中值写回 `opencode.jsonc` 顶层 `model` / `small_model`
  （官方 schema 为 snake_case，research 已确认）。
- Provider 管理卡片：列出 `opencode.jsonc` 中全部 provider（名称、npm SDK、
  baseURL、模型数、启用开关→`whitelist` 语义本期不做，仅展示）；
  「刷新目录」重新读盘；「连接 Provider」本期为占位（`message.info` 提示
  手动编辑路径，避免误写用户凭据）。
- apiKey 展示默认掩码（`sk-1i…8A` 形式），提供「显示」切换；编辑 apiKey 后
  保存写回原文件。

### R5 OpenCode 详情 — 权限设置
- 按官方 schema 的 permission 键（read / edit / glob / grep / list / bash /
  webfetch / ...以 schema 实际枚举为准）渲染 `ask|allow|deny` 三态选择
  （naive-ui NSelect 或分段控件），写回 `opencode.jsonc` 的 `permission` 对象。
- 未设置过的键显示「默认（继承）」态，选择后才写入。

### R6 OpenCode 详情 — 环境变量
- KEY=VALUE 行编辑器（行列表 + 增删按钮 + 多行兜底 textarea 切换），保存写入
  **应用级** `~/.lightr-vibe/agent-env.json`（官方 schema 无全局 env 对象，
  research 已确认；provider 级 `env` 仅为变量名清单，不承载值）。
- 说明文案明示：该文件供 Lightr-vibe 后续启动 Agent 进程时注入，不改动
  opencode 自身配置。
- 「保存环境变量」按钮 + 成功/失败 message 反馈。

### R7 配置读写的正确性与安全
- JSONC 读写必须**保留注释与格式**：前端用 `jsonc-parser` 的 `modify()`
  生成最小编辑补丁（保留注释/键序/缩进），Rust 侧只负责读原文/写补丁结果。
- 写前自动备份：`opencode.jsonc.bak-<timestamp>`（同目录，最多保留 5 份）。
- 写冲突保护：读时记录 mtime，写前 Rust 校验，不一致返回 conflict 错误，
  前端提示「配置已被外部修改」并提供重新加载。
- apiKey 默认掩码显示；本应用不上传、不打印密钥内容。
- 所有文件/进程操作仅在 Tauri 环境执行；浏览器环境整体回退 mock 并明示。
- 并发保护：前端单例写锁，写失败保留编辑态并报错。

### R8 Rust 侧命令（不新增前端 capability）
- 新增模块 `src-tauri/src/agent.rs`，注册 commands：
  - `agent_detect_opencode` → which/PATH 解析 + `--version` + `acp --help` 探测
    （spawn 失败回退 `cmd /C`，见 research §1）
  - `agent_read_opencode_config` → 原文 + mtime + 路径
  - `agent_write_opencode_config { text, expected_mtime }` → mtime 校验、备份、写回
  - `agent_read_env_file` / `agent_write_env_file` → `~/.lightr-vibe/agent-env.json`
- 全部走 Rust `std::process` / `std::fs`，**不修改 capabilities/default.json**，
  不引入 tauri-plugin-fs。

## Acceptance Criteria

1. `pnpm build` 通过（vue-tsc + vite）；`cargo check`（src-tauri）通过。
2. Tauri 环境：设置页可从 TopBar 齿轮与 Home 进入；Agent 列表选中 OpenCode
   后显示真实版本（本机 1.18.35）与 4 项预检 PASS/FAIL 真实结果。
3. 修改主模型/小模型/权限并保存后，`~/.config/opencode/opencode.jsonc`
   中对应键更新，**原有注释、键序与缩进不变**；同目录生成 .bak 备份。
4. 保存期间外部修改该文件（手动改 mtime 场景）：保存返回冲突提示，不覆盖。
5. 环境变量保存后 `~/.lightr-vibe/agent-env.json` 内容正确；重进设置页回显。
6. 未安装 OpenCode 的机器（或临时改 PATH 测试）：列表显示 FAIL/未安装徽章，
   详情显示安装引导，不崩溃。
7. 浏览器 `pnpm dev`：设置页可用且顶部有 mock 提示条，所有操作不报错。
8. 设计图其余设置菜单项与 Agent 行均为禁用/占位态，点击有中文反馈，无死链。
9. UI 符合 `ui-style.md`：白底、neutral 灰阶、blue-500 强调、rounded-md、
   text-xs/[13px]、无阴影、中文文案。

## Out of Scope（本期不做）

- ACP 会话驱动（用 OpenCode 跑真实对话）——后续任务。
- 除 OpenCode 外任何 Agent 的真实检测/配置。
- 设计图其余设置页（外观/常规/MCP/Skills/模型供应商/快捷键 等）的真实功能。
- Provider 的在线连接/凭据录入向导（本期占位）。
- 设置项的跨设备同步、导入导出；agent-sidecar 的任何改动。
- capabilities 变更与 tauri-plugin-fs 引入。

## Deferred / Risks

- D1 version-fox shim 为 `.cmd`：Rust CreateProcess 不可直接执行 → 已定
  `cmd /C` 回退链（research §1）。
- D2 opencode 官方 schema 版本漂移：写入键以 research §3 快照为准；若用户
  本机 opencode 版本 schema 不同，写回仍为合法 JSONC，opencode 会忽略未知键。
- D3 全局 env 不存在于官方 schema → 环境变量落应用级文件（R6），语义在 UI
  文案中说明，避免误导用户以为 opencode 会读取。
- R1 风险：jsonc-parser 补丁对极端格式（CRLF/无尾换行）的保真度 → implement
  阶段以本机真实文件做往返测试（read→modify→write→diff 仅目标键）。
