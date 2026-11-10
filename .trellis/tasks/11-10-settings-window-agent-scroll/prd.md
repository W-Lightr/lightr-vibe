# 设置窗口化与 Agent 管理独立滚动（含 ACP 适配器版本核查）

## Goal

1. 设置界面从「当前窗口内跳路由」改为**独立 Tauri 窗口**，且同一时刻只存在一个设置窗口（再次点击设置时聚焦已有窗口）。
2. Agent 管理页的**左列 Agent 列表与右列详情各自独立滚动**，外层页面不再整体滚动。
3. 核查 ACP 适配器（`claude-agent-acp`、`codex-acp`、`opencode-ai`）最新版本与兼容性，给出是否升级的明确结论。

## Background

- 当前 `/settings` 挂在 `WorkspaceShell` 下，侧栏底部设置按钮、首页齿轮、以及
  `useConversation.passGate()` 的「去设置」都通过 `router.push('/settings')` 在同一窗口内跳转。
- `SettingsView.vue` 的 `<main>` 使用 `overflow-y-auto`，右列详情没有独立滚动容器；内容超高时
  连同左侧 Agent 列表一起被外层滚动，列表的 `overflow-y-auto` 实际不生效。
- 本仓库不依赖任何 ACP npm 包；Rust 侧手写 JSON-RPC 客户端，`initialize` 固定发送
  `protocolVersion: 1`。适配器由用户 `npm i -g` 自装，App 只探测并拉起。

## Requirements

### R1 独立单例设置窗口

- 桌面端点击任意设置入口 → 新开一个设置窗口，不再改变主窗口当前页面。
- 设置窗口已存在时：`show` + `unminimize` + `setFocus`，不重复创建。
- 浏览器 dev（非 Tauri）保持现状：回退到页内 `/settings` 路由，保证界面可开发。
- 窗口创建失败要有可见反馈，不能静默失败。

### R2 设置窗口形态

- 设置窗口为**独立形态**：只渲染设置页（左侧设置分区菜单 + 右侧内容），不携带
  `WorkspaceShell` 的主应用侧栏。
- 保留无边框窗口体验：顶部 header 为拖拽区，右侧保留窗口三键。
- 设置窗口的返回按钮语义 = **关闭设置窗口**；页内 `/settings`（浏览器 dev）仍为返回上一页/首页。

### R3 所有设置入口统一

- 侧栏底部设置按钮、首页右上角齿轮、`passGate` 的「去设置」都必须走同一个入口函数。
- 不允许再出现新增的 `router.push('/settings')` 直接跳转（浏览器 fallback 除外，且集中在入口函数内）。

### R4 Agent 管理左右独立滚动

- 左列：Agent 列表超出高度时在列表区域内滚动；底部「默认引擎」「子代理委派」保持固定可见。
- 右列：详情内容超出高度时在右列区域内滚动。
- 外层 `<main>` 不再产生整页滚动条。

### R5 ACP 适配器版本核查

- 记录三个包的 latest / 本机已装版本与发布时间。
- 给出「是否升级、升级到哪个版本、为什么」的结论，并写清与我们 ACP v1 客户端假设的冲突点。
- 本次**不**执行 `npm i -g`（不改动用户全局环境）；只产出结论与后续动作建议。

## Constraints

- 不修改 ACP 协议实现；不实现 ACP v2。
- 设置窗口不得重复安装编码工作区的全局启动副作用（ACP/委派事件监听、任务看板桥接），
  避免两个 WebView 同时写同一份会话/委派数据。
- 保留 `/settings` 页内路由用于浏览器 dev 回退。
- 遵循 `.trellis/spec/frontend/tauri-bridge.md`：窗口三键每屏只出现一次；环境相关 API 走 `getAppWindow()`。

## Acceptance Criteria

- [ ] AC1 桌面端从侧栏、首页齿轮、`passGate` 三处打开设置，均弹出独立设置窗口；主窗口停在原页面。
- [ ] AC2 设置窗口已打开时再次触发设置入口，只聚焦该窗口，窗口数量不增加。
- [ ] AC3 设置窗口只显示设置页（无主应用侧栏），顶部可拖拽、窗口三键可用、返回按钮关闭窗口。
- [ ] AC4 浏览器 dev 点击设置仍在当前页进入 `/settings`，不报错。
- [ ] AC5 Agent 列表项足够多时，仅左列出现滚动条；「默认引擎」「子代理委派」不被滚走。
- [ ] AC6 右列详情内容超高时，仅右列出现滚动条；外层页面无滚动条。
- [ ] AC7 `pnpm build`（`vue-tsc --noEmit` + `vite build`）通过；Tauri capability 配置能通过启动校验。
- [ ] AC8 ACP 版本核查结论记录在任务 `research/acp-versions.md`，包含明确升级建议。

## Out of Scope

- 升级/回滚用户的全局适配器安装。
- ACP v2 客户端支持与协议层改造。
- 设置页其他分区的功能实现。
- 设置窗口与主窗口的父子生命周期联动（关闭主窗口不强制关闭设置窗口）。

## Notes

- ACP 结论摘要：`claude-agent-acp` **0.103.0 起为 BREAKING，仅服务 ACP v2 并移除 v1 桥**，
  与本项目 `protocolVersion: 1` 冲突；建议停在 **0.102.0**（最后一个 v1 兼容版本）或维持 0.90.0。
  `opencode-ai` 1.19.0 → 1.19.5 为补丁升级，可升。`codex-acp` 本机未安装，最新 2.6.0，需要真机冒烟后再定。
