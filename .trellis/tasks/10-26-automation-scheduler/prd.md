# 复刻 codeg 自动化（定时任务）到 lightr-vibe

## Goal

把 codeg 的「自动化」能力搬进 lightr-vibe：把一段已经写好的提示词 + 引擎/模型/目录
保存成一条**自动化**，按 cron 定时（或手动）触发，每次触发**产出一条运行记录**；
运行可以是「无头会话直接跑完」或「往任务看板塞一张 todo 卡」。

参照实现：`H:\AI\Light-vibe\codeg\src-tauri\src\automation\`（引擎 + 调度）、
`src-tauri\src\commands\automation.rs`（CRUD 命令面）、`src-tauri\src\models\automation.rs`
（数据形状）、`src-tauri\src\db\service\automation_service.rs`（cron 求值 / CAS 抢占 /
结算 / 回收）、`src\components\automations\*`（前端页面）。

本仓库现状约束（决定了必须偏离 codeg 的地方）：

- 无数据库。任务看板是 `~/.lightr-vibe/lightr-vibe/tasks.json` 单文件 JSON，
  只经 `src-tauri/src/task.rs` 的 `task_*` 命令改写。
- ACP 会话是**进程内内存**态（`src-tauri/src/acp.rs` 的 `CONNS`），不落盘；
  进程重启后既没有 live 连接，也无法把旧会话「续」成一次运行。
- 任务的启动链路在**前端**（`src/views/tasks/taskSessionBridge.ts::startTask`），
  Rust 侧没有、也不应该有拉起任务的代码。

## Requirements

### R1 自动化数据与 CRUD

- 自动化持久化到 `~/.lightr-vibe/lightr-vibe/automations.json`（新文件，与
  `tasks.json` / `settings.json` 同目录、同样走 app namespace 路径解析与
  「损坏先备份 `.bak` 再写」的保护）。
- 字段（对齐 codeg `automation` 表，命名用 camelCase 上线）：
  `id` / `name` / `enabled` / `triggerKind('schedule'|'manual')` / `cron` /
  `timezone` / `nextRunAt` / `engineId` / `projectId` / `modelId?` /
  `effortId?` / `effortValue?` / `useWorktree`(默认 true) / `baseBranch?` /
  `initCommand?` / `action('launch_session'|'enqueue_task')` /
  `prompt` / `lastRunAt?` / `lastRunStatus?` / `lastRunId?` / `unseenFailures` /
  `createdAt` / `updatedAt` / `deletedAt?`。
- 命令：`automation_list` / `automation_get` / `automation_create` /
  `automation_update` / `automation_set_enabled` / `automation_delete`
  （软删）/ `automation_mark_seen` / `automation_compute_next_run`（编辑器的
  「下次运行」预览，与调度器共用同一个 cron 求值器）。
- 保存时校验，**不合法即拒绝保存**（错误在编辑器里立刻可见，而不是变成一次
  失败的运行）：名称非空；提示词非空；`projectId` 必填；
  `triggerKind='schedule'` 时 `cron` 必填且可解析、`timezone` 合法；
  `action='enqueue_task'` 时 `projectId` 必填。

### R2 调度（cron + 手动）

- 5 字段 cron（分/时/日/月/周），在 `timezone` 指定的 IANA 时区求值，落库一律
  UTC 毫秒。周字段按「0/7=周日，1=周一…6=周六」的 UI 习惯接受并适配底层实现
  （codeg 踩过这个 off-by-one）。
- 进程内调度器每 30s 轮询一次：取 `enabled && triggerKind=schedule &&
  nextRunAt <= now && deletedAt is null` 的自动化，**CAS 抢占**当前时间片
  （把 `nextRunAt` 从读到的旧值条件更新为 `now` 之后的下一时刻），抢到的才真正
  触发。这样桌面端 + 服务端同用一份数据、或重启后被补触发，都只有一次。
- `nextRunAt` 永远从 `now` 向前重算：停机跨越 10 个时点，恢复后只补触发 1 次，
  不补 10 次。
- 手动「立即运行」`automation_run_now(id)` 绕过 cron，触发的是一次
  `trigger='manual'` 的运行。
- 禁用（`enabled=false`）、`triggerKind=manual`、软删的自动化都不参与调度。

### R3 一次运行的生命周期

- 运行状态：`running` / `succeeded` / `failed` / `cancelled` / `skipped`。
  `skipped` 是「上一条还没跑完就又到点」被抑制掉的那次。
- **同一自动化永不并发**：每条自动化一把进程内 fire 锁，把
  「检查是否已有 running」+「插入 running 记录」+「启动」串行化；已有 running
  时本次记为 `skipped` 并返回 `previous run still active`。
- 启动前任何失败（引擎不可用 / 目录不存在 / 提示词为空）都必须把这次运行
  结算成 `failed` 并带上可见的 `error`，**绝不允许静默挂住**。
- `enqueue_task` 是同步完成的：把提示词（连同自动化的引擎/模型/工作树偏好）
  建成一张 `todo` 任务，然后本次运行立刻结算为 `succeeded`，`summary` 写
  `queued task <id>: <title>`。
- `launch_session` 是异步完成的：真正起 ACP 会话、跑完一个 turn 后按
  `stopReason` 结算（`end_turn → succeeded`，`cancelled → cancelled`，其余 →
  `failed`），并抓取末轮 assistant 文本作为 `summary`。
- `automation_cancel_run(run_id)`：把一个在飞的运行结算成 `cancelled`，并终止
  它对应的连接（若已建立）。

### R4 运行记录与历史

- 每次运行一条记录：`id` / `automationId` / `status` / `trigger`
  (`'schedule'|'manual'`) / `scheduledFor?` / `startedAt?` / `endedAt?` /
  `sessionId?` / `taskId?` / `stopReason?` / `error?` / `summary?` / `createdAt`。
- 运行历史随自动化一起持久化，按 `createdAt` 倒序取最近 N 条（默认 20）。
- 保留期 30 天：每 6 小时剪一次旧记录（与 codeg 同口径）。
- 失败会累加父自动化的 `unseenFailures`；打开自动化页时
  `automation_mark_seen` 清零，侧栏徽标据此显示。

### R5 进程重启恢复

- 启动时（拿到「单引擎锁」之后）把**所有** `running` 记录一次性结算为
  `failed`，`error = "interrupted by restart"`，**绝不在这里补触发**。
  这条激进恢复只有在确认自己是唯一引擎时才是安全的，所以用
  `automations.json.lock` 的独占文件锁（跨平台 `flock`/`LockFileEx`，
  进程退出由 OS 释放）来证明；抢不到锁的进程不启动引擎，手动 run/cancel
  返回 `automation engine not running`。
- 运行中的周期性对账（每 30s）：本进程在跟的运行由完成事件结算；连接已消失的
  按会话终态兜底结算，兜不住就 `failed("run lost its worker")`；非本进程运行的
  按会话终态兜底，超过 180 分钟上限强制 `failed`。

### R6 前端页面

- 新路由 `/automations`（独立页，`views/automations/AutomationsView.vue`），
  侧栏 `SidebarPanel` 增加一项「自动化」（`lucide-vue-next` 图标），
  活跃态与「任务」一致。
- 列表：一行一条自动化，展示名称、启用开关、触发描述（「每天 09:00 ·
  Asia/Shanghai」/「仅手动」）、下次运行、最近运行状态、失败徽标；
  行内快捷操作：立即运行、取消在飞运行、编辑、删除。
- 编辑器（弹窗，风格对齐 `TaskEditorDialog.vue`）：名称、提示词、目标项目、
  引擎、模型、思考强度、动作（起无头会话 / 塞任务卡）、工作树开关 + 基线分支 +
  初始化命令、触发方式（手动 / cron 表达式 + 时区），并实时显示「下次运行」。
- 运行历史：抽屉/弹层，列出该自动化的运行记录（状态、触发、耗时、摘要、
  错误），`enqueue_task` 的运行可跳转任务看板，`launch_session` 的运行可跳转
  对应会话。
- 所有后端访问经 `src/services/automationService.ts`（`isTauri()` 双通道 +
  `automationMock.ts`），组件/store 不直接 `invoke`。

### R7 launch_session 与前端会话系统的边界（关键约束）

`launch_session` 需要 Rust 侧**自己**拉起 ACP 会话，但前端 `useConversation`
维护着会话/消息的单份真相。为不制造两个真相：

- Rust 侧为一次运行创建**一个新的本地 session id**（`auto-<runId>-<rand>`），
  与前端 `sessionStore` 的 id 空间不相交；该会话只存在于运行时，不写入前端的
  会话索引。
- 完成的判定**不依赖**前端事件总线：`acp_prompt` 本身就是一个阻塞返回
  `stopReason` 的调用，引擎在自己的 `spawn_blocking` 任务里等它返回即可，
  天然不会漏事件、不需要 `connection_id` 索引。
- 会话的实时消息在前端不可见（这是可接受的取舍，记录在 design 里）。
  运行历史里保留 `sessionId`，若用户想复盘，前端打开该运行详情时用
  `acp_session_load` 把它当只读会话载入（**前提**是该 agent 支持 loadSession；
  不支持时该项返回 `null` 并在 UI 上说明「该会话不可回放」）。

## Acceptance Criteria

- [ ] AC1 新建一条 `triggerKind=schedule` 且 cron 为「每分钟」的自动化，等一个
      时点，运行历史自动出现一条 `trigger=schedule` 的记录；`nextRunAt` 已推进。
- [ ] AC2 同一条自动化在上一条仍 `running` 时再次到点，产生一条 `skipped`
      记录，且没有第二条 `running`。
- [ ] AC3 `triggerKind=manual` 的自动化永不自动触发；`automation_run_now` 可让它
      立刻产生一条 `trigger=manual` 的运行。
- [ ] AC4 禁用后 `nextRunAt` 被清空且不再触发；重新启用后 `nextRunAt` 重算为下次。
- [ ] AC5 `automation_compute_next_run` 的返回值与真实触发时刻一致（周字段
      `1` = 周一，`7` = 周日）。
- [ ] AC6 `action=enqueue_task` 触发后：看板上出现一张 `todo` 任务，标题为自动化
      名称，提示词/引擎/模型/工作树设置一致，运行记录 `succeeded` 且 `taskId` 指向它。
- [ ] AC7 `action=launch_session` 触发后：agent 真的收到提示词并跑完，运行记录按
      `stopReason` 结算并带 `summary`；`acp_cancel`（经 `automation_cancel_run`）
      能让运行变成 `cancelled`。
- [ ] AC8 引擎不可用 / 项目已删除 / 提示词为空等启动前失败，运行记录为 `failed`
      且有可读的 `error`，不存在永远 `running` 的记录。
- [ ] AC9 强杀进程再启动：之前所有 `running` 记录变成
      `failed("interrupted by restart")`，且没有因此补触发。
- [ ] AC10 两个进程指向同一份数据目录时，只有一个跑调度器；另一个的
      `automation_run_now` 返回 `automation engine not running`。
- [ ] AC11 `/automations` 页面在浏览器 dev（mock）与 Tauri 两端都能完成
      列表 / 新建 / 编辑 / 启停 / 立即运行 / 查看历史。
- [ ] AC12 `automations.json` 损坏时：读取退化为空，损坏原文先备份成
      `automations.json.bak`，下一次写入不丢原文。
- [ ] AC13 `cargo test`（新增的 cron / CAS / 结算 / 恢复单测）与
      `pnpm build`（`vue-tsc --noEmit` + `vite build`）通过。

## Out of Scope

- 不做 codeg 的 `shared_in_root`（根仓库串行 checkout 分支）隔离模式：
  本仓库的隔离只复用任务自身的 `useWorktree`。
- 不做自动化模板库（codeg `template-gallery.tsx`）、不做 `label_snapshot`
  这类展示用快照。
- 不做 codeg 的 server / 多端共享（`web::event_bridge`、Axum handler）。
- 不做跨自动化依赖 / DAG 编排。

## Notes

- 保留 codeg 的三个核心不变量，它们是这套设计真正值钱的地方：
  1. **CAS 抢占时间片**——多进程 / 重启都不会漏触发或重触发；
  2. **fire 锁串行化「查重 + 插入 + 启动」**——手动与定时撞车不会起两条；
  3. **独占锁证明自己是唯一引擎**——才敢在启动时把 `running` 一律判死。
- 与 codeg 的差异（都因本仓库无 DB、会话不落盘）写在 `design.md` §2。
