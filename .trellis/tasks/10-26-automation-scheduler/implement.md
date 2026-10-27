# Implement — 自动化（定时任务）

按 `design.md` 执行。每一步都能独立验证；打 ✅ 前必须跑该步的验证命令。

验证命令（本仓库无 lint 脚本，用这两个）：

```
cd src-tauri; cargo test
pnpm build            # 含 vue-tsc --noEmit + vite build
```

---

## 阶段 0 · 前置检查

- [x] 0.1 读 `src-tauri/src/task.rs` 的 `TaskStore` / `with_file_write` /
      `save_serialized` / `create_id` / 测试隔离手法（`point_store_at`），
      确认新 `store.rs` 与之同构。
- [x] 0.2 读 `src-tauri/src/acp.rs` 的 `connect_inner` / `rpc_call` /
      `disconnect_conn` / `handle_client_request`，确认
      `launch_session` 可以复用哪些内部函数、哪些需要 `pub(crate)`。
- [x] 0.3 读 `src/views/tasks/components/TaskEditorDialog.vue` 与
      `TaskDetailDialog.vue`，作为新编辑器/历史弹窗的样式基准。
- [x] 0.4 确认 `SidebarPanel.vue` 的 `NavKey`/`navItems`/`onNavClick` 结构。

---

## 阶段 1 · Rust 数据层（无调度）

- [x] 1.1 新建 `src-tauri/src/automation/mod.rs`，声明子模块与 `pub use`。
- [x] 1.2 新建 `models.rs`（或直接写在 `store.rs` 顶部）：`Automation` /
      `AutomationRun` / `AutomationDraft` / `TriggerKind` / `AutomationAction` /
      `AutomationRunStatus`，全部 `#[serde(rename_all = "camelCase")]`、
      字段 `#[serde(default)]`。
- [x] 1.3 新建 `store.rs`：`AutomationFile { version, automations, runs }`、
      `AutomationStore`（path/data 双 Mutex 全局单例）、
      `load_from_disk` / `save_serialized` / `with_file_write` /
      `backup_corrupt`。`.bak` 必须在覆盖**之前**写。
- [x] 1.4 测试隔离：`LIGHTR_AUTOMATION_TEST_PATH` + `point_store_at` /
      `reset_store` / `lock_store`。
- [x] 1.5 写 store 单测（缺文件 / 损坏 `.bak` 顺序 / 未知键保留 / 往返）。
- [x] **验证**：`cargo test automation::store`

## 阶段 2 · cron 求值

- [x] 2.1 新建 `cron.rs`：
      - `TZ_TABLE: &[(&str, i32 /*offset min*/, bool /*has_dst*/)]` +
        `tz_offset_minutes(tz) -> (i32, bool)`
      - `remap_dow_field(field) -> Result<String, String>`
      - `normalize_cron(expr) -> Result<String, String>`（5 字段接受，6/7 拒绝）
      - `compute_next_run(cron, tz, after_ms) -> Result<Option<i64>, String>`
        （分钟步进，`MAX_SEARCH_DAYS = 366*4`，dom/dow 皆非通配时取 OR）
- [x] 2.2 单测覆盖 design §9 第 1、2 条（含周一/周日、工作日预设、
      2 月 30 日、时区偏移）。
- [x] **验证**：`cargo test automation::cron`

## 阶段 3 · service（纯业务，无 tokio）

- [x] 3.1 `validate_draft(draft) -> Result<(), String>`：名称 / 提示词 /
      projectId / rootPath / cron+tz（schedule）/ action 约束。
- [x] 3.2 CRUD：`list` / `get`（过滤 `deletedAt`）/ `create` / `update` /
      `set_enabled`（禁用清 `nextRunAt`，启用重算）/ `soft_delete`。
- [x] 3.3 `next_run_for(draft, now)`：仅在 `enabled && triggerKind==schedule`
      时计算，否则 `None`。
- [x] 3.4 调度原语：`list_due(now)` / `claim_due(id, now)`（CAS：
      读旧 `nextRunAt` → 用 `now` 重算 → 条件更新，affected != 1 即失败）。
- [x] 3.5 运行原语：`has_active_run` / `start_run` / `record_skipped_run` /
      `settle_run`（幂等：已是终态返回 false；失败时 `unseenFailures + 1`；
      同时写父行 `lastRunAt`/`lastRunStatus`/`lastRunId`）/
      `list_runs(id, limit)` / `list_active_runs`。
- [x] 3.6 维护：`boot_reconcile_interrupted` / `prune_old_runs(days)` /
      `mark_all_seen`。
- [x] 3.7 单测覆盖 design §9 第 3–7 条。
- [x] **验证**：`cargo test automation::service`

## 阶段 4 · task 建卡入口抽取

- [x] 4.1 `task.rs`：把 `task_create` 的命令体抽成
      `pub(crate) fn create_task_internal(draft: TaskDraft) -> Result<TaskDetails, String>`，
      命令壳改为一行转调。**行为零变化**（现有 task 测试必须全绿）。
- [x] 4.2 `task.rs`：新增 `pub(crate) fn get_task(id) -> Option<WorkTask>`
      （若自动化需要在结算时回查标题，可选）。
- [x] **验证**：`cargo test task::`

## 阶段 5 · ACP 复用面

- [x] 5.1 `acp.rs`：`AcpConn` 增加 `owner: ConnOwner`（`User | Automation`，
      默认 `User`）。加 `pub(crate) fn spawn_automation_conn(...) -> Result<String, String>`
      （复用 `connect_inner`，把 owner 标成 `Automation`）。
- [x] 5.2 `acp.rs`：`handle_client_request` 的 `session/request_permission`
      分支——**owner 是 Automation 时**在 Rust 侧直接回
      `allow_once`（取 `kinds` 里第一个 `allow_once`，回退 `allow_always`，
      都没有则拒绝），不再挂进 `pending_perm`。
      **不得**按 engine 名分支（spec：`rg "claude|codex" acp.rs` 只允许命中测试）。
- [x] 5.3 `acp.rs`：把 `session_new` / `set_config` / `prompt` 的**内核**
      抽成 `pub(crate)` 版本（不依赖 `tauri::State`，参数直给），
      命令壳转调。行为零变化。
- [x] 5.4 `acp.rs`：`pub(crate) fn shutdown_automation_conn(conn_id)` =
      `acp_cancel` + `disconnect_conn`。
- [x] **验证**：`cargo test acp::`

## 阶段 6 · 引擎

- [x] 6.1 `engine.rs`：`Ownership` 枚举 + `acquire_engine_ownership`
      （`automations.json.lock`，std 独占，`Unavailable`/`Taken` 都 fail closed）。
- [x] 6.2 `engine.rs`：`AutomationEngine` { store, app_handle, in_flight,
      fire_locks, _engine_lock }，`OnceLock` 全局 + `engine()`。
- [x] 6.3 `engine.rs`：`launch_enqueue_task(auto, run_id)`（调
      `task::create_task_internal`，同步结算 `succeeded`）。
- [x] 6.4 `engine.rs`：`launch_session(auto, run_id)`（`spawn_blocking`：
      spawn conn → session_new → set_config(model/effort) → prompt →
      按 `stopReason` 结算 → 无论成败 disconnect）。
- [x] 6.5 `engine.rs`：`run_automation`（fire 锁 + `has_active_run` →
      `skipped` + `start_run` + `emit run_started` + launch + 失败即 `failed`）。
- [x] 6.6 `engine.rs`：`cancel_run`（先结算 cancelled → emit → 取 fire 锁 →
      拆连接）。
- [x] 6.7 `engine.rs`：`reconcile_once`（in-flight 之外且超 180 分钟的
      running → `failed("run lost its worker")`）。
- [x] 6.8 `engine.rs`：`run_engine` 主循环（30s 调度 / 30s 对账 / 6h 剪枝，
      `MissedTickBehavior::Delay`，触发 `tokio::spawn`）。
- [x] 6.9 `engine.rs`：`boot_reconcile_interrupted` 在拿锁之后、spawn 循环之前
      调用。
- [x] 6.10 单测：引擎锁互斥；`enqueue_task` 分支端到端（不打真 agent）；
      `run_automation` 的 skipped 路径。
- [x] **验证**：`cargo test automation`

## 阶段 7 · 命令与注册

- [x] 7.1 `automation/commands.rs`：11 个命令（design §6 列表）。
- [x] 7.2 `lib.rs`：`mod automation;` + `invoke_handler` 注册 11 个命令 +
      `setup(|app| { automation::engine::boot(app.handle().clone()); Ok(()) })`。
- [x] 7.3 `lib.rs` 退出路径：`RunEvent::Exit` 里除了 `acp::shutdown()`
      不再加东西（锁由文件句柄 Drop 释放）。
- [x] **验证**：`cargo build`；`cargo test`

## 阶段 8 · 前端类型 / service / mock

- [x] 8.1 `src/types/automation.ts`：类型 + `automationErrorMessage()` +
      `ACTION_LABELS` 常量。
- [x] 8.2 `src/services/automationService.ts`：11 个方法，`isTauri()` 双通道，
      `automation://changed` 事件的 listen 辅助（`onAutomationChanged`）。
- [x] 8.3 `src/services/automationMock.ts`：内存列表 + CRUD + 简化
      `computeNextRun` + `runNow`（建假任务卡，status succeeded）。
      文件头注明「mock cron 不追求与 Rust 等价」。
- [x] **验证**：`pnpm build`（类型必须过）

## 阶段 9 · 前端 store 与页面

- [x] 9.1 `src/stores/automation.ts`：state + 全部动作（design §7.3）。
- [x] 9.2 `views/automations/scheduleLabel.ts`：cron+tz → 人话；单测（若
      vitest 可用）。
- [x] 9.3 `AutomationCard.vue`：展示 + 行内操作 + `NPopconfirm` 删除。
- [x] 9.4 `AutomationEditorDialog.vue`：表单 + 校验 + 「下次运行」节流预览 +
      项目/引擎/模型联动（复用 `engineStore`、`ensureModelOptions`）。
- [x] 9.5 `CronBuilderDialog.vue`：预设 + 自定义表达式。
- [x] 9.6 `RunHistoryDialog.vue`：运行列表 + 跳任务 / 跳会话 / 取消。
- [x] 9.7 `AutomationsView.vue`：列表 + 新建 + 事件订阅 + `markSeen`。
- [x] 9.8 `router/index.ts`：`/automations` 路由。
- [x] 9.9 `SidebarPanel.vue`：`navItems` 增加「自动化」项 + 跳转分支。
- [x] **验证**：`pnpm build`；`pnpm dev` 手工走查 mock 通道

## 阶段 10 · 端到端验收

- [ ] 10.1 `dev:tauri` 启动 → AC1（每分钟自动化自动触发一条记录）。
- [ ] 10.2 AC2（运行中再到点 → skipped）。
- [ ] 10.3 AC3（manual 不自动跑；Run now 可跑）。
- [ ] 10.4 AC4（禁用清 nextRunAt / 启用重算）。
- [ ] 10.5 AC6（enqueue_task → 看板出现 todo 卡，字段一致）。
- [ ] 10.6 AC7（launch_session 真跑完，按 stopReason 结算；cancel 生效）。
- [ ] 10.7 AC8（项目目录删掉 → failed 且错误可读）。
- [ ] 10.8 AC9（任务管理器强杀 → 重启后 running 全变 failed）。
- [ ] 10.9 AC10（双开两个实例 → 只有一个调度器）。
- [ ] 10.10 AC12（手工写坏 `automations.json` → `.bak` + 空列表 + 后续写入）。

## 阶段 11 · 收尾

- [x] 11.1 `.trellis/spec/frontend/tauri-bridge.md` 增补一节
      「Automation commands + 调度引擎不变量」（CAS 抢占 / fire 锁 /
      独占锁 / boot reconcile / 自动放行权限）。
- [x] 11.2 全量 `cargo test` + `pnpm build` 通过。
- [ ] 11.3 提交（等用户确认）。

---

## Review Gate

阶段 1–7（Rust）完成后暂停，请用户确认数据模型与引擎不变量，再进前端。
阶段 8–9（前端）完成后暂停，请用户过一遍 UI，再做端到端验收。

## 回滚点

- 阶段 1–3 只新增文件，删掉即可。
- 阶段 4–5 改动既有 `task.rs` / `acp.rs`：这是**唯一**动既有可能回归的地方，
  该阶段结束必须 `cargo test` 全绿后才继续。回滚方式是恢复这两个文件的
  命令壳（内部函数留着无害）。
- 阶段 6–7 只新增 + `lib.rs` 注册，回滚 = 撤注册 + 删模块。
- 阶段 8–9 回滚 = 删页面 + 撤路由/侧栏 + 删 service/store/types。
