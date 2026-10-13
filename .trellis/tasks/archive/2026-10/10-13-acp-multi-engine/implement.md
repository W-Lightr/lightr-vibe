# Implement: 多引擎 ACP 抽象与新建会话选引擎

上下文顺序：`prd.md` → `design.md` → `.trellis/spec/frontend/tauri-bridge.md`（ACP 段）
→ 父任务归档产物 `archive/2026-10/10-08-opencode-acp-chat/`。
参考实现（只读）：`H:\AI\Light-vibe\codeg`（路径见 prd.md Background）。
数据命名空间硬规则：仅 `~/.lightr-vibe/lightr-vibe/`。

## 有序清单

### A. Rust 引擎身份与注册表（照 codeg）
1. 新建 `src-tauri/src/agent_type.rs`：`AgentType` 枚举（仅 `OpenCode`）+ `BUILTIN_AGENT_TYPES`
   + `as_wire`/`from_wire`/`Display`/`Serialize`/`Deserialize`（照 codeg `models/agent.rs:23`）。
   加「wire 名不可变」单测。
2. 新建 `src-tauri/src/engine.rs`：`AcpAgentMeta` + `LaunchSpec` + `ContextWindowSource`
   + `get_agent_meta(AgentType) -> AcpAgentMeta`（照 codeg `acp/registry.rs:512`）+
   `AcpAgentInfo`（camelCase）+ `list_agents()`。
3. 把现状常量搬入 opencode meta：`acp.rs:133-206` 的 bin 候选/subcommand/env_strip；
   `agent.rs:76,103,193,201` 的检测/配置；`acp.rs:943-1079` 的 context-window 规则。
4. `lib.rs` 加 `mod agent_type; mod engine;`；`cargo check`。

### B. Rust 命令面引擎化
5. `acp.rs`：`resolve_bin`/`build_acp_command`/`resolve_spawn` 改为 `&AcpAgentMeta` 入参；
   `AcpConn` + `engine: AgentType`；`acp_connect(sessionId, engineId)` 解析 wire。
6. `acp.rs`：`probe_options_inner(cwd, meta)`、`model_context_window_inner(model, meta)`；
   命令 `acp_probe_options(cwd?, engineId?)`、`agent_model_context_window(model, engineId?)`。
7. `agent.rs`：`agent_detect_opencode` → `agent_detect(engineId)`；
   `detect_opencode_inner` → `detect_inner(meta)`；配置读/写按 meta.config_path。
   `agent_read/write_opencode_config` → `agent_read/write_config(engineId)`。
8. 新增 `acp_list_agents`（照 codeg `acp_list_agents_core`）→ `list_agents()`。
9. 新增 `engine_default_get`/`engine_default_set`（读写 `settings.json`）。
10. `lib.rs` 更新 invoke_handler；`cargo check` + `cargo test --lib`。

### C. 前端服务/存储
11. `src/services/engineService.ts`（新）：`list()`（acp_list_agents）/`defaultGet()`/
    `defaultSet(id)` + isTauri 双通道 mock。
12. `src/services/acpService.ts`：`connect(sessionId, engineId)`、`probeOptions(cwd?, engineId?)`、
    `modelContextWindow(provider, model, engineId?)`（**注意现状签名是 `(provider, model)`**）；
    `detect(engineId)`。
13. `src/services/opencodeService.ts`：detect/config 调用改走引擎参数（或并入 engineService）。
14. `src/stores/engine.ts`（新，照 codeg `hooks/use-acp-agents.ts`）：单一共享 store，
    `agents`/`fresh`/`defaultEngineId`/`load()`/`refresh()`；排序、失败保留、静态回退。
15. `src/stores/agent.ts`：单 `detectResult` → 按 AgentType 键的 map；调用点按 engineId。
16. `src/stores/session.ts`：`Session`/`SessionIndexEntry` + `engineId`；
    `createSession(model, {engineId,…})`；`toIndexEntry`/`sessionFromEntry`/`forkSession`。
    **补 `seedFolderSession`（`:17`）**：其 `createSession` 调用（`:19`）需带 `engineId`，
    否则种子会话落一条 `engineId: undefined` 脏记录（AC10）。

### D. 前端消费面
17. `src/composables/useConversation.ts`：`MODEL_AGENT_KEY` 常量 → `session.engineId`；
    auth/门禁文案按引擎；`ensureAcpSession` 建连传 `session.engineId`。
18. `src/views/coding/components/NewSessionDialog.vue`（新）：引擎下拉（默认 defaultEngineId，
    不可用 fallback 首个 available，不写回默认）→ 确认创建。ui-style 合规。
19. 新建会话入口接弹窗：`SidebarPanel.vue`（folder/tmp 两处）、`CodingWorkspace.vue`、
    `TopBar.vue`。
20. `MessageComposer.vue`：agentName 兜底/占位文案按会话引擎。
21. `AgentManageView.vue`：列表由 `stores/engine` 渲染、`selectedAgentId` 响应式、
    加「默认引擎」选择写 settings；`OpencodeDetail` 保持。

### E. Mock
22. `src/services/acpMock.ts` / `opencodeMock.ts`：补 `acp_list_agents`/`engine_default_*`、
    detect 的 engineId 形状；保证浏览器 dev 全流程不崩。

### F. 验证
23. `cargo check` + `cargo test --lib` + `npx vue-tsc --noEmit`。
24. Tauri 实机 AC3：新建 opencode 会话 → 发送/流式/工具/停止/权限/模型切换/重启恢复
    与改造前一致。
25. AC4（重定义）：前端 mock 验「弹窗选择 → engineId 落库」链路 + Rust 单测验注册表契约；
    **不 spawn 假引擎**。
26. AC5：手工造一条无 engineId 的旧 index → 加载默认 opencode 可继续。
27. AC6：设置页列表注册表驱动 + 默认引擎项生效。
28. AC7：浏览器 dev mock 全流程。
29. AC8：`rg -i "opencode" src-tauri/src` 核查协议路由无引擎名分支。
30. AC9/AC10：Rust 单测通过；种子会话带 engineId。
31. trellis-check → 提交。

## 验证命令

```bash
cd src-tauri && cargo check && cargo test --lib
npx vue-tsc --noEmit
rg -i "opencode" src-tauri/src/acp.rs src-tauri/src/lib.rs   # AC8 残留核查
pnpm dev:tauri   # AC3/AC4/AC5/AC6
pnpm dev         # AC7
```

## 风险/回滚

- **命令改名连锁**：`agent_detect_opencode` 等被前端引用，先 `rg` 全量定位再改；一次改净。
- **两处 resolve_bin 重复**（`acp.rs:130` / `agent.rs:100`，`agent.rs:128-129` 注释称有意）
  → 收敛到 `engine.rs`。
- **建连顺序**：engineId 必须在 `acp_connect` 前确定（会话已存 engineId）；旧会话默认 open_code。
- **wire 契约不可变**：`"open_code"` 落盘后不可改（单测锁）。
- **过度分支警戒**：出现多个 `if agent_type == …` 时改为数据化。
- 回滚：git 整体 revert 单任务 diff（删 `agent_type.rs`/`engine.rs` + 还原常量 + 去弹窗）。

## start 前检查
- [ ] `implement.jsonl` / `check.jsonl` 填入真实 spec/research 条目
- [ ] 用户批准最终规划摘要
