# Implement: OpenCode ACP 真实对话（子任务 2）

上下文顺序：`research/acp-protocol.md` → `research/opencode-acp-probe.md` →
`prd.md` → `design.md` → tauri-bridge.md + frontend specs。
顺序说明：本子任务可在子任务 1 之前或之后实施；cwd 先用
`~/.lightr-vibe/tmpsession`（appData 既有约定）即可独立跑通。

## 有序清单

### A. Rust ACP 层
1. `src-tauri/src/acp.rs`：进程解析/spawn（真身 exe + CREATE_NO_WINDOW +
   .cmd 回退）、读线程 JSON-RPC 路由（response/notification/client-request
   三分）、pending map + 超时矩阵、emit 三事件、7 命令（design §2）。
2. `lib.rs` 注册；`cargo check`。
3. 用 `research/probes/` 脚本对跑：Rust 进程与 python 探针握手/new/prompt/
   cancel 输出一致性抽查。

### B. 前端服务层
4. `src/services/acpService.ts`（invoke+listen 单例、状态机 ready/dead、
   超时包装）；`src/services/acpMock.ts`（MockEvent→update 适配）。
5. `src/composables/useConversation.ts` 重写（门禁/映射/停止/落盘触发）。
6. `src/composables/useAcpPermission.ts`（队列 + 三模式策略 + 总是允许 map）。

### C. 持久化
7. `src/stores/session.ts`：索引与消息落盘 hooks、懒加载、load/重连路径、
   标题规则；数据目录 `~/.lightr-vibe/lightr-vibe/`。

### D. UI
8. `MessageComposer.vue` 模型下拉会话级化（configOptions 优先、MODELS 兜底）。
9. `PermissionCard.vue` 新组件 + `ConversationView.vue` 挂载（ui-style 合规：
   neutral 卡 + blue 主按钮 + red 拒绝 + rounded-md + text-[13px]）。
10. `MessageItem.vue`/`ToolCallPanel.vue`/`TokenUsagePanel.vue` 兼容真实形状
    （累积替换语义、usage 真实值、thinking 变体）。
11. 错误卡：ACP dead / 凭据失败 / timeout → 消息内错误卡 + 「去设置」按钮。

### E. 验证
12. `pnpm build` + `cargo check`。
13. Tauri 实机 AC2-7（真实流式/停止/三模式权限/模型切换/重启恢复/无凭据形态）。
14. 进程死亡注入（taskkill opencode）→ dead → error 卡 → acp_start 恢复。
15. 浏览器 mock AC8。
16. 命名空间 AC9。
17. trellis-check 后提交。

## 验证命令

```bash
pnpm build && cd src-tauri && cargo check
node .trellis/tasks/10-08-opencode-acp-chat/research/probes/<probe>.py  # 协议对跑
pnpm dev:tauri   # AC2-7, AC9
pnpm dev         # AC8
```

## 风险/回滚

- 长驻进程资源：acp_stop 于窗口关闭（App.vue onBeforeUnmount 不可靠 →
  Tauri RunEvent::Exit 钩子 Rust 侧清理）。
- 8s session/new 首调：UI 新建会话按钮 loading 态 ≥ 该时长不假死（30s 超时
  后错误卡）。
- 凭据失败形态多样（-32602 / prompt 阶段 error）：统一映射错误卡，不细分。
- 回滚：useConversation 单文件指回旧消费面；acp.rs 整模块可摘除（lib.rs 两行）。

## start 前检查
- [ ] implement.jsonl/check.jsonl 已填
- [ ] 父任务决策并入（Q1/Q2/权限 UX）
- [ ] 用户批准最终摘要

---

## 追加：每会话一进程 + 空闲断连（design §8，后续迭代）

> 顺序：先改 Rust 进程仓库，再改 service/前端，最后接空闲 sweep 与关标签杀进程。

### F. Rust：单进程 → 每会话一进程（`src-tauri/src/acp.rs`）
1. `CONNS: Mutex<HashMap<String, Arc<AcpConn>>>` 替换 `conn_slot()`；`AcpConn` 加
   `last_activity_at`。`current_conn`/`require_conn` → 改为按 connId 取
   （`get_conn(connId)` / `require_conn(connId)`）。
2. `acp_connect(session_id)`：per-session 启动锁（Map<sessionId, Mutex>），幂等；
   spawn（`build_acp_command`）+ initialize 握手；事件 emit 带 `connId`。
3. 读线程/`reader_loop`/`mark_dead`/`handle_incoming`：全部改为 per-conn（emit 带
   connId；dead 只清该 conn）。
4. 命令面签名加 connId：`acp_session_new/load/prompt/cancel/set_config/
   respond_permission/session_close`；新增 `acp_disconnect(session_id)`（关 stdin→等2s→kill）。
5. 事件 payload 加 `connId`：`acp://update`/`acp://permission`/`acp://status`。
6. `shutdown()` 遍历 CONNS 全部关闭。
7. `lib.rs` 更新 handler 注册（含 `acp_connect`/`acp_disconnect`）。
8. `cargo check` + 用 probes 脚本抽查握手/new/prompt/cancel。

### G. 空闲断连（Rust）
9. `LIGHTR_ACP_IDLE_TIMEOUT_SECS`（默认 180，0 关闭）；后台定时线程每 ~15s 扫描，
   断连「connected && 非 generating && 无 pending permission && 空闲超时」的 conn
   （`acp_disconnect`）。活动刷新 `last_activity_at`（prompt/update/permission）。

### H. 前端（`src/services/*`、`src/composables/useConversation.ts`）
10. `acpService`：`connect(sessionId)`、`disconnect(sessionId)`；其余命令加 connId
    （浏览器 mock 同步改造，形状一致）。
11. `useConversation`：`liveAcpSessions` 改以**本地会话 id** 为键；事件订阅改为按
    分发的 `connId` 路由；`ensureConnected` → `acpService.connect`；`closeConnection`
    → `acp_disconnect`（关标签即杀进程）；生成中先 cancel。
12. 健康芯片：`sessionConnected` = 本地会话 id ∈ live（每会话独立）；沿用连接中/绿/异常/未连。

### I. 验证
13. `pnpm build` + `cargo check`。
14. Tauri：打开两个会话 → 任务管理器见两个 opencode 进程；关其一出、另一个不受影响；
    空闲 180s 后进程自动退出；重开/发送时自动重连（load 恢复）。
15. 浏览器 mock AC8 不回归。

### 回滚
- `acp.rs` 还原单例 `conn_slot`；service/前端签名同步还原。
