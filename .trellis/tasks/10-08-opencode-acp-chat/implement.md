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
