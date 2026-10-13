# Journal - lightr (Part 1)

> AI development session journal
> Started: 2026-10-08

---



## Session 1: ACP 每会话一进程 + 空闲断连（per-session 架构）
<!-- trellis-session: v=2 fp=5658e7d8c47620f0 -->

**Date**: 2026-10-13
**Task**: ACP 每会话一进程 + 空闲断连（per-session 架构）
**Branch**: `main`

### Summary

将 ACP 从单长驻进程多会话改造为每会话独立进程+连接（对齐 codeg）：CONNS<connId,AcpConn> 仓库、connect/disconnect 命令、idle sweep（180s 可配）、事件与命令按 connId 路由、关标签杀进程。trellis-check 全 8 项不变量 PASS，自修 5 处缺陷（StatusResult connId、死 API findByAcpSessionId、toolCallHosts 复合键、健康芯片按会话、过期注释）。cargo check/cargo test(15)/vue-tsc 全绿。待办：AC10-12 需 Tauri 实机点验（两进程互不影响/关标签杀进程/空闲自动退出）。后续需求：多引擎 ACP 抽象（完整插件化，新建会话选引擎，会话模型加 engineId）。

### Git Commits

| Hash | Message |
|------|---------|
| `573d2f2` | feat(workspace): ACP 每会话一进程 + 空闲断连 + UI 精修 |

### Status

[OK] **Completed**


## Session 2: 多引擎 ACP 抽象 + 新建会话选引擎（照 codeg 裁剪）
<!-- trellis-session: v=2 fp=443e7bd7fefc3676 -->

**Date**: 2026-10-13
**Task**: 多引擎 ACP 抽象 + 新建会话选引擎（照 codeg 裁剪）
**Branch**: `main`

### Summary

把硬编码 opencode 的 ACP 层改造为可插拔引擎抽象，照 codeg 三主结构裁剪（砍下载/安装/自定义/DB）。Rust: agent_type.rs（AgentType 枚举 + as_wire/from_wire 持久化契约，wire 名单测锁死）+ engine.rs（AcpAgentMeta/LaunchSpec 注册表 + get_agent_meta + list_agents + bare_bin_name）。acp.rs/agent.rs: spawn/probe/context-window/整条 detect 链按 meta 引擎化；AcpConn 加 engine；协议路由无引擎名分支（AC8）。命令面: acp_connect(sessionId,engineId)/acp_probe_options/agent_model_context_window(provider,model,engineId?)/agent_detect/read/write_config + 新增 acp_list_agents/engine_default_get/set。前端: types/engine.ts + engineService.ts + stores/engine.ts（单一共享 store、fallback 不写回默认）+ NewSessionDialog.vue 弹窗（接入三处新建入口）+ session engineId（旧数据默认 open_code）+ AgentManageView 注册表驱动+默认引擎。trellis-check 8 项不变量 PASS，自修 2 处（cmd /C 回退用 bare_bin_name 保 PATHEXT、过期命令名注释）。cargo check/cargo test(24)/vue-tsc 全绿。latent: agent.ts activeEngineId 未接线，多引擎时需补（已记入 spec 与 commit）。

### Git Commits

| Hash | Message |
|------|---------|
| `5a7782a` | feat(agent): 多引擎 ACP 抽象 + 新建会话选引擎（照 codeg 裁剪） |

### Status

[OK] **Completed**
