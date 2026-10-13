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
