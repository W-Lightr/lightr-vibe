# Design — 自动化（定时任务）

对应 `prd.md`。技术设计与取舍；执行顺序见 `implement.md`。

---

## 1. 边界与模块划分

```
src-tauri/src/
├── automation/
│   ├── mod.rs          # pub use；单引擎全局句柄
│   ├── store.rs        # automations.json 读写（AutomationStore，Mutex 全局单例）
│   ├── cron.rs         # 纯函数：normalize_cron / compute_next_run / 周字段重映射
│   ├── service.rs      # 纯业务：校验、CAS 抢占、start/settle/skip/prune/mark_seen
│   ├── engine.rs       # 调度循环 + fire 锁 + launch_session + cancel + reconcile
│   └── commands.rs     # #[tauri::command] 薄壳（只做 State/AppHandle 解包）
├── lib.rs              # 注册命令 + setup 里 spawn 引擎
└── task.rs             # 新增 pub(crate) 内部建卡入口（复用现有落盘）

src/
├── types/automation.ts              # 领域类型（与 Rust camelCase 逐字段对齐）
├── services/automationService.ts    # 唯一 invoke 入口，isTauri 双通道
├── services/automationMock.ts       # 浏览器 dev 内存实现
├── stores/automation.ts             # Pinia setup store（列表 / 选中 / 历史缓存）
├── views/automations/
│   ├── AutomationsView.vue
│   ├── scheduleLabel.ts             # 纯函数：cron + tz → 「每天 09:00」
│   └── components/
│       ├── AutomationCard.vue
│       ├── AutomationEditorDialog.vue
│       ├── CronBuilderDialog.vue
│       └── RunHistoryDialog.vue
└── views/coding/components/SidebarPanel.vue   # navItems 增加一项
```

**分层纪律**（沿用仓库既有约定）：

- 组件/store 只经 `automationService`；`invoke` 只出现在 service 里。
- `cron.rs` 与 `scheduleLabel.ts` 是纯函数，可直接单测。
- `engine.rs` 是唯一持有 tokio 定时任务的地方；`service.rs` 不感知 tokio。

---

## 2. 与 codeg 的差异（以及为什么）

| codeg | 本仓库 | 原因 |
|---|---|---|
| SeaORM + SQLite `automation` / `automation_run` 表 | `automations.json` 单文件 `{version, automations[], runs[]}` | 无 DB；与 `tasks.json` 一致的落盘栈 |
| `TurnComplete` 事件 + `connection_id → run_id` 内存索引结算 | 直接 `spawn_blocking(acp_prompt)`，**等返回值**结算 | `acp_prompt` 同步返回 `stopReason`，天然不丢事件，索引与对账兜底都不需要 |
| 会话落库、`conversation_id` 供 UI 打开 | 运行只记 `sessionId`（运行期内存会话） | 本仓库会话在 `CONNS` 内存，不入索引 |
| `isolation = worktree_per_run / shared_in_root` | 单一 `useWorktree` + `baseBranch` + `initCommand`，透传给建出的任务 | 复用 `worktree.rs` 与任务自己的开关，避免 Rust 侧第二次实现 git |
| `enqueue_task` → work_task 引擎自己跑 | `enqueue_task` → 建 `todo` 任务，执行仍由 `taskSessionBridge` 负责 | 启动链路在前端，不在 Rust |
| 一个 engine 同时服务桌面/服务端 | 仅桌面 Tauri 进程 | 无 server 模式；独占锁仍保留（双开桌面） |
| `config` 是一坨 JSON 快照（prompt blocks + label cache） | `prompt: String` 平铺字段 | 本仓库编辑器只有纯文本提示词，没有 block 化 composer |

保留的不变量：**CAS 抢占时间片**、**fire 锁串行化查重+插入+启动**、
**独占文件锁证明唯一引擎**、**启动时一律判死 `running`**、**先结算再拆连接**。

---

## 3. 数据模型

### 3.1 `automations.json`

```jsonc
{
  "version": 1,
  "automations": [
    {
      "id": "auto-1a2b3c4d",
      "name": "每日依赖巡检",
      "enabled": true,
      "triggerKind": "schedule",            // "schedule" | "manual"
      "cron": "0 9 * * *",                  // schedule 时必填（5 字段，UI 语义）
      "timezone": "Asia/Shanghai",
      "nextRunAt": 1793000000000,           // UTC 毫秒；null = 不再触发
      "engineId": "open_code",
      "projectId": "proj-xxxx",
      "modelId": "magpie/workbuddy-ai/deepseek-v4.1-flash",
      "effortId": null,
      "effortValue": null,
      "prompt": "检查依赖过期情况并汇总",
      "action": "enqueue_task",             // "launch_session" | "enqueue_task"
      "useWorktree": true,
      "baseBranch": null,
      "initCommand": null,
      "lastRunAt": 1792900000000,
      "lastRunStatus": "succeeded",
      "lastRunId": "arun-9f8e",
      "unseenFailures": 0,
      "createdAt": 1792800000000,
      "updatedAt": 1792900000000,
      "deletedAt": null
    }
  ],
  "runs": [
    {
      "id": "arun-9f8e",
      "automationId": "auto-1a2b3c4d",
      "status": "succeeded",                // running|succeeded|failed|cancelled|skipped
      "trigger": "schedule",                // "schedule" | "manual"
      "scheduledFor": 1792896000000,        // 本次消费的时间片；manual 为 null
      "startedAt": 1792900000000,
      "endedAt": 1792900045000,
      "sessionId": null,                    // launch_session 时的运行时会话 id
      "taskId": "task-77aa",                // enqueue_task 时建出的任务
      "stopReason": "end_turn",
      "error": null,
      "summary": "queued task task-77aa: 每日依赖巡检",
      "createdAt": 1792900000000
    }
  ]
}
```

- 全部 `#[serde(default)]`，缺字段即默认值（codeg 的 AC9 同款容忍度），
  旧文件能直接读。
- 时间统一 `i64` UTC 毫秒（与 `task.rs` 一致，不引 `chrono`）。
- `deletedAt` 软删：列表默认过滤，运行历史保留（`run` 不随自动化删除）。

### 3.2 落盘策略（复用 `task.rs` 的模式）

- 全局 `OnceLock<AutomationStore>`；`path` 与 `data` 各一把 `Mutex`。
- 读：文件不存在 → 空数据，**不创建**；读失败或 JSON 损坏 → 空数据 + 把损坏
  原文写到 `automations.json.bak`，**写发生在下一次覆盖之前**。
- 写：先序列化到字符串，再 `write` 临时文件 + rename（沿用 `task.rs`
  的 `save_serialized` 思路），写失败回滚内存副本。
- 未知顶层键在读写往返中保留（与 `tasks.json` 同约定）。
- 测试用 `LIGHTR_AUTOMATION_TEST_PATH` 环境变量 + `point_store_at()` /
  `reset_store()` / `lock_store()`，与 `task.rs` 测试隔离手法一致。

---

## 4. cron 求值（`cron.rs`）

### 4.1 依赖决策：自实现，不引 crate

选型对比：

| 方案 | 优点 | 缺点 |
|---|---|---|
| `cron` crate | 现成、支持 6/7 字段 | 引入 + 需 `chrono`；时区要靠 `chrono-tz`（数据量大）；周字段语义仍是它自己的 |
| `chrono` + `chrono-tz` + 自算 | 无 | 两个新依赖，`chrono-tz` 单独就几百 KB |
| **纯 std 自实现（选）** | 零新依赖，5 字段全支持，时区用「固定偏移表 + 注册表」 | 需要自己处理 DST 和闰年 |

本仓库当前离线可用、无 DB，`timezone` 只需要**常见 IANA 名到 UTC 偏移**的解析。
实现为一张固定表（`Asia/Shanghai → +08:00 无 DST`、`Asia/Tokyo`、`Asia/Kolkata`、
`Europe/London`、`Europe/Berlin`、`America/New_York`、`America/Los_Angeles`、
`UTC`）+ 默认回退 `UTC`。

> **取舍写明**：对有 DST 的时区，本版本按**标准时偏移**求值，不做夏令时切换。
> 这对本应用的主要用户（`Asia/Shanghai`，无 DST）无影响；落在表外的时区一律按
> UTC 求值，并在编辑器里提示「该时区暂不支持夏令时，按 UTC 求值」。表中每一项
> 都是一个 `(name, offset_minutes, has_dst)`，`has_dst=true` 的项在 UI 上打提示。
> 这是 codeg（`chrono-tz` 精确）与本实现的**唯一已知语义差**，记录在此以免将来
> 被当成 bug。

### 4.2 算法

```
normalize_cron(expr, tz_offset) -> Result<String>
  - trim；按空白切分
  - 5 字段：分 时 日 月 周   → 周字段重映射（保持 UI 语义，见 4.3）
  - 6/7 字段：拒绝（本仓库不支持秒级/年级）

compute_next_run(cron, tz, after_utc_ms) -> Result<Option<i64>>
  local = after_utc_ms + tz_offset           // 换成本地「墙上时间」
  从 local 所在分钟 + 1 分钟开始，按分钟步进，最多搜 4 年（约 210 万步上限，
  实际命中天文数字之前就返回），对每个候选检查：
     minute 匹配 && hour 匹配 && dom 匹配 && month 匹配 && dow 匹配
  dom 与 dow 同时「非通配」时，按 Vixie cron 语义取 **OR**
  命中即转回 UTC 毫秒返回；越界无解 → None
```

- 步进单位是分钟（cron 最小粒度），用纯整数运算，无浮点。
- 搜索上限用 `MAX_SEARCH_DAYS = 366 * 4`，超出返回 `None`（与 codeg
  「no future occurrence」同语义），避免 `0 0 30 2 *` 这类永不命中的死循环。
- `normalize_cron` 把结果**规范化后存库**？**不存**。存用户输入的 5 字段原文，
  求值时再规范，这样 UI 能原样回显（codeg 也是存原文、求值前 normalize）。

### 4.3 周字段重映射（codeg 踩过的坑）

UI 语义是 `0|7=周日, 1=周一 … 6=周六`（`Date.getDay()` 一致），而「1=周一」
与某些实现「1=周日」差一天。本实现内部统一用 `0=周日..6=周六`：

```
remap_dow(field):
  "*" → "*"
  每项 n（含 a-b、a/n、逗号列表）：
     n == 7 → 0
     0 <= n <= 6 → n
     其他 → Err("周字段取值超出 0-7")
  重映射后若出现重复项则去重（"1-7" → "0-6"）
```

单测必须覆盖 `1`=周一、`7`=周日、`0`=周日、「工作日 `1-5`」不含周末、
「`0 9 * * 1`」落在周一。

### 4.4 预览与触发同源

`automation_compute_next_run(cron, timezone)` 命令和调度器调用**同一个**
`compute_next_run`。编辑器预览因此不可能与实际触发时刻分叉（codeg 的
`automation_compute_next_run_core` 同款保证）。

---

## 5. 调度引擎（`engine.rs`）

### 5.1 单引擎独占锁

```
acquire_engine_ownership(data_dir) -> Exclusive(File) | Taken | Unavailable
  path = data_dir.join("automations.json.lock")
  打开（create + write，不 truncate）→ try_lock()（std 跨平台独占）
  Ok                      → Exclusive(文件句柄保活到进程退出)
  WouldBlock              → Taken（另一个 live 进程持有）→ 不启动引擎
  其他 IO 错误            → Unavailable → 不启动引擎（fail closed）
```

- 锁文件是**纯锁句柄**，不写内容，不 truncate。
- OS 在进程退出/崩溃时释放，因此下一次启动能正确拿到锁并做激进恢复。
- 抢不到锁 → `engine()` 全局句柄保持 `None`：
  手动 run/cancel 命令返回 `automation engine not running`，
  CRUD 命令**照常可用**（用户可以编辑自动化，只是这台进程不驱动它们）。

### 5.2 启动序列（`setup` 钩子里）

```
1. acquire_engine_ownership()
   - 失败 → 记录日志，不 spawn，返回
2. boot_reconcile_interrupted()   // 所有 running → failed("interrupted by restart")
3. 建立全局句柄 AUTOMATION_ENGINE
4. tauri::async_runtime::spawn(run_engine(engine))
```

> 顺序关键：**先拿锁再判死**。若先判死再拿锁，两个进程同时启动时后到的那个
> 会把前者正在跑的记录误判死。

### 5.3 主循环

```
let mut bus = ticker(30s)      // 调度轮询
let mut reconcile = ticker(30s)
let mut prune = ticker(6h)
loop {
  select! {
    _ = bus.tick() => {
      for id in list_due(now) {
        // CAS 抢占；抢到的才触发
        match claim_due(id, now) {
          Some(slot) => tokio::spawn(run_automation(id, "schedule", Some(slot))),
          None => {}                          // 被别人抢到 / 已不再到期
        }
      }
    }
    _ = reconcile.tick() => reconcile_once().await,
    _ = prune.tick()     => prune_old_runs(30 days),
  }
}
```

- `MissedTickBehavior::Delay`：机器休眠/卡顿后不会连补多次 tick。
- 触发在 `tokio::spawn` 里跑：worktree 创建 + 起进程可能要几秒，不能阻塞
  调度与对账两个 tick。
- 首次 tick 立即执行（`interval` 的第一次 tick 是即时的），相当于启动后马上
  补一次到期检查。

### 5.4 触发（`run_automation`）

```
run_automation(automation_id, trigger, scheduled_for) -> Result<run_id, String>
  fire_lock = 该自动化的锁
  guard = fire_lock.lock().await            // ← 串行化「查重+插入+启动」

  auto = store.get(automation_id)?          // not-found / 已软删 → Err
  if service.has_active_run(auto.id) {
      service.record_skipped_run(...)       // 记一条 skipped
      emit_changed(auto.id)
      return Err("previous run still active")
  }

  run = service.start_run(auto.id, trigger, scheduled_for)?   // running
  emit_run_started(auto.id, run.id)         // 立刻广播，UI 不必等 launch

  match launch(auto, run.id).await {
      Ok(())  => Ok(run.id),
      Err(e)  => {
          service.settle_run(run.id, Failed, None, Some(e.clone()), None)
          emit_run_settled(auto.id, run.id, "failed")
          Err(e)
      }
  }
```

- `start_run` 同时把父自动化的 `lastRunAt` / `lastRunStatus` 写成
  `running`，列表上立刻能看到「运行中」。
- `launch` 失败**必须**结算成 `failed`：不允许 `running` 挂到天荒地老
  （对账兜底是给「本进程不再跟」的异常路径用的，不是给正常错误用的）。

### 5.5 `launch`：两条分支

```
launch(auto, run_id):
  if auto.action == EnqueueTask:   return enqueue_task(auto, run_id)
  return launch_session(auto, run_id)
```

**A) `enqueue_task`（同步结算）**

```
1. project = projectStore.getPathFor(auto.projectId)  ← 见 5.9「项目目录从哪来」
2. draft = TaskDraft {
     projectId, title = first_chars(auto.name, 80), prompt = auto.prompt,
     engineId = auto.engineId, modelId, effortId, effortValue,
     useWorktree = Some(auto.useWorktree), baseBranch, initCommand,
   }
3. task = task::create_task_internal(draft)?      // 复用 task_create 的落盘逻辑
4. settle_run(run_id, Succeeded, "queued", None,
              summary = "queued task {id}: {title}")
5. emit_run_settled(auto.id, run_id, "succeeded")
```

- 不留 worktree 给自动化自己建：`useWorktree` 只是**写进任务卡片**的默认值，
  真正的 `worktree add` 由 `taskSessionBridge.startTask` 在用户/自动启动该任务时
  执行 —— 这与「用户在任务编辑器里勾上工作树」完全同一条路径，不新增分支。
- `task::create_task_internal` 是**新增的 `pub(crate)` 函数**，把现有
  `task_create` 命令的函数体抽出来，命令壳改成调用它。这样建卡沿用
  `tasks.json` 的 id/事件/排序/落盘，不复制第二份写入逻辑。

**B) `launch_session`（异步结算）**

```
1. ensure no cancel raced in:  re-read run; status != running → Ok(()) 直接返回
2. cwd = project.rootPath                     // 工作树由任务负责，无头会话不建
3. engine_ok = engine::resolve_spawn(engineId) — 不可用即 Err(可读提示)
4. conn_id = "auto-{run_id}-{rand}"           // 与前端 session id 空间不相交
5. spawn_blocking:
     connect_inner(app, conn_id, agentType)   // 复用 acp.rs 现有内部实现
     session_new(conn_id, cwd)
     set_config(model / effort)               // 复用 acp_set_config 的内核
     prompt_result = acp_prompt_core(conn_id, acp_session_id, auto.prompt)
6. 结算：
     stopReason "end_turn"  → Succeeded
     stopReason "cancelled" → Cancelled
     其他 / Err             → Failed(error)
     summary = 末轮 assistant 文本（若有）
7. 无论成功失败：disconnect_conn(conn_id, "automation")  // 一个 turn 一个进程
8. emit_run_settled(...)
```

- **权限请求**：无头会话没人点「允许」。做法是给运行会话一个
  **`auto-approve` 权限策略**：`handle_client_request` 收到
  `session/request_permission` 时，若该 conn 被标记为 automation 会话，
  直接在 Rust 侧回 `allow_once`（取 `kinds` 里第一个 `allow_once`，没有则
  `allow_always`，都没有则拒绝）。标记挂在 `AcpConn` 上
  （`owner: ConnOwner::Automation`），**不按 engine 名分支**。
  > 这是本设计里唯一新增的安全面：自动化会话自动放行工具权限。写进编辑器
  > 的提示文案（「无头会话会自动允许工具调用」），并在 PRD 的 R7 里记明。
- **会话可见性**：前端看不到这条会话的实时消息（前端未 connect 它）。
  运行历史里点「查看会话」时，前端**新开一个会话壳**并
  `acp_session_load(connId', sessionId, cwd)` 回放；agent 不支持
  `loadSession` 则返回 `null`，UI 显示「该会话不可回放」。

### 5.6 取消（`cancel_run`）

```
1. settle_run(run_id, Cancelled, stop_reason="cancelled")   // 先结算（CAS）
   若已结算（false）→ 说明竞态，继续拆连接但不重复发事件
2. emit_run_settled(auto_id, run_id, "cancelled")           // 立刻让 UI 离开 running
3. 取该自动化的 fire_lock（与 launch 互斥）
   ← 保证 teardown 要么发生在 prompt 之前（launch 的 status 检查会中止），
     要么发生在 turn 真在飞时（cancel 掉真 turn），不会穿插进 prompt 入队
4. 找到该 run 的 sessionId/conn_id：
   - launch_session 的 run：acp_cancel(conn) 然后 disconnect_conn(conn)
   - enqueue_task 的 run：没有会话，settle 完就结束（若已建出 task 卡，
     卡片留在看板上，由用户决定是否取消）
```

### 5.7 对账（`reconcile_once`，每 30s）

本仓库的结算由 `launch` 自己完成（prompt 返回值），所以对账只剩两件事：

```
for run in list_active_runs() {           // status == running
  if 本进程跟踪中 (in-flight 表里有 run_id) → 跳过（launch 会结算它）
  else {
    // 本进程不知道它（可能上一个进程留下的，但 boot 已清；或异常丢失）
    if startedAt 早于 180 分钟 → settle Failed("run lost its worker")
    // 剩下情况：留着等 launch 结算（正常情况下 in-flight 表会有它）
  }
}
```

> 与 codeg 的差别：codeg 需要「从产物会话的终态反推」因为会话在 DB 里；
> 本仓库的 `acp_prompt` 是阻塞返回，正常路径根本走不到对账。对账只用于
> 「in-flight 表丢了」这种程序性错误的兜底，因此**只需要 180 分钟上限**。

### 5.8 in-flight 表

`Arc<Mutex<HashMap<String /*runId*/, InFlight>>>`，`InFlight { conn_id: Option<String>, started_at: i64 }`。

- `launch_session` 在 spawn 前插入，结算后移除（`finally` 语义，用 guard 结构）。
- `enqueue_task` 是同步的，短暂插入即可（对账里表现为「跟踪中」）。
- 进程重启时表为空 → 启动对账已把 `running` 全判死，无残留。

### 5.9 项目目录从哪来

自动化只存 `projectId`，而 `cwd` 需要真实路径。前端 `projectStore` 有
`rootPath`。三个选项：

1. **前端把 rootPath 写进自动化**：数据冗余，项目改名/移动即失效。
2. **新增 `project_resolve(projectId) -> rootPath` 命令**：需要 Rust 侧知道项目
   列表存哪；项目当前是前端持久化的。
3. **自动化里存 `rootPath` 快照 + 每次触发前校验目录存在**（选）。

选 3：`automation` 字段改为 `projectId` + `rootPath`（创建/编辑时由编辑器
从 `projectStore` 填入并展示，只读）。触发时：

- 目录不存在 → `failed("project folder not found: <path>")`，**不重建、不猜**。
- 编辑器里项目被移除 → 编辑器要求重新选择项目后才能保存。

这比「让 Rust 去读前端的状态」简单且不会产生第二个真相源。

### 5.10 事件

跟 `task.rs::broadcast` 同款：`app.emit("automation://changed", { id })`。

- `created` / `updated` / `enabled` / `deleted` / `run_started` / `run_settled`
  都发同一事件名，payload 只带 `{ id, runId? }`，前端收到就**重读整表**。
- 理由：自动化数量级是几十，重读比增量补丁简单得多，也不会漏字段
  （codeg 前端也是「每次事件重拉 run 列表」）。

---

## 6. 命令面（`commands.rs`）

全部 camelCase 结果、`Result<T, String>`；错误串沿用仓库词汇
（`not-found` / `io: ...` / `automation engine not running`）。

```rust
automation_list()                                   -> Automation[]
automation_get(id)                                  -> Automation
automation_create(draft)                            -> Automation
automation_update(id, draft)                        -> Automation
automation_set_enabled(id, enabled)                 -> Automation
automation_delete(id)                               -> ()
automation_mark_seen()                              -> ()
automation_compute_next_run(cron, timezone)         -> Option<i64>
automation_runs(id, limit)                          -> AutomationRun[]
automation_run_now(id)                              -> String   // runId
automation_cancel_run(runId)                        -> ()
```

内部（非命令）：

```rust
pub(crate) fn boot_reconcile_interrupted() -> u64
pub(crate) fn run_automation(id, trigger, scheduled_for) -> Result<String, String>
pub(crate) fn cancel_run(run_id) -> Result<(), String>
```

---

## 7. 前端设计

### 7.1 类型（`types/automation.ts`）

与 Rust 逐字段对齐（`Automation` / `AutomationRun` / `AutomationDraft` /
`AutomationAction` / `TriggerKind` / `AutomationRunStatus`）。`nextRunAt` 等
时间字段是 `number | null`（毫秒）。`automationErrorMessage(err)` 做错误串
兜底（对齐 `types/task.ts::taskErrorMessage`）。

### 7.2 service

`automationService` 每个方法 `if (!isTauri()) return mock*()` 后 `invoke`。
`automationMock.ts` 内存实现：列表、CRUD、一个假的 `compute_next_run`
（简化：只支持 `* * * * *` / `0 H * * *` / `0 H * * N` 三类，其余返回 null），
以及一个手动 `runNow` 直接把 run 标成 `succeeded` 并建一张假任务卡。

> mock 的 cron 只求覆盖 UI 演示路径，**不追求与 Rust 等价**（浏览器 dev 没有
> 真调度器）。在 mock 文件头写清这一点。

### 7.3 store

`useAutomationStore`（setup store）：
`items`、`loaded`、`loading`、`runsById: Record<string, AutomationRun[]>`、
`load()` / `refresh()` / `create()` / `update()` / `setEnabled()` / `remove()` /
`runNow()` / `cancelRun()` / `loadRuns(id)` / `markSeen()`。

- 每个写操作 `await` 后用**返回值**替换行（不乐观更新）。
- `automation://changed` 事件在 `AutomationsView` 挂载时监听、卸载时移除；
  收到后调 `refresh()`（不是增量合并）。
- 未读失败数 = `items.reduce((s, a) => s + a.unseenFailures, 0)`；
   进入页面 `markSeen()` 后本地立即清零并刷新。

### 7.4 页面

`AutomationsView.vue`（naive-ui）：

- 顶部：标题、`新建自动化` 按钮、刷新。
- 主体：`AutomationCard` 列表（空态引导）。
- 卡片：名称 + 启用 `NSwitch`、`scheduleLabel(cron, tz)`、下次运行
  （`formatRelativeTime` 复用 `utils/time.ts`）、最近状态 chip、失败徽标、
  行内操作（立即运行 / 取消 / 编辑 / 历史 / 删除 `NPopconfirm`）。
- 编辑器 `AutomationEditorDialog.vue`（`NModal` + `NForm`）：
  名称、提示词（`NInput type=textarea`）、项目（下拉来自
  `projectStore`，选中后写 `projectId` + `rootPath`）、引擎（`engineStore`
  的已装引擎）、模型（`ensureModelOptions` 同款探针）、思考强度（可选）、
  动作（`NRadioGroup`）、工作树开关 + 基线分支 + 初始化命令、触发方式
  （手动 / cron）、时区。cron 输入旁挂「下次运行：…」—每次输入节流调用
  `automation_compute_next_run`，返回 null 时红字提示「该表达式不会再触发」。
- `CronBuilderDialog.vue`：常用预设（每小时 / 每天 H 点 / 每周 N / 每月 1 号）
  + 自定义表达式；确定后回填到编辑器。
- `RunHistoryDialog.vue`：`NDataTable` 或列表，列 = 时间 / 触发 / 状态 /
  耗时 / 摘要 / 操作（查看任务 / 查看会话 / 取消）。

### 7.5 侧栏

`SidebarPanel.vue` 的 `navItems` 加 `{ key: 'automation', label: '自动化',
icon: Clock, to: '/automations' }`（`automation` 这个 key 与 `NavKey` 联合类型
已存在，只需补 `to` 与路由跳转分支）。路由表加 `/automations`。

---

## 8. 边界情况与失败矩阵

| 情况 | 行为 |
|---|---|
| `automations.json` 不存在 | 空列表，不报错，不建文件 |
| JSON 损坏 | 读为空 + `.bak` 备份（覆盖前写）；下次写不丢原文 |
| 抢不到引擎锁 | 不 spawn；CRUD 可用；run/cancel → `automation engine not running` |
| 启动前项目目录不存在 | 运行 `failed("project folder not found: …")` |
| 提示词为空 | 保存时拒绝（`io: 提示词不能为空`）；即便历史脏数据，触发时也 `failed` |
| 引擎未安装 | 运行 `failed(<NotInstalled 的可读 hint>)` |
| 上一条还在跑 | 本次记 `skipped`，`run_now` 返回 `previous run still active` |
| 触发瞬间用户点了取消 | fire 锁串行化；launch 在起进程前重读 status，已取消则静默返回 |
| 无头会话遇到权限请求 | 自动 `allow_once`（第一个 `kinds` 匹配项） |
| agent 半路崩 | `acp_prompt` 返回 Err / `stopReason != end_turn` → `failed` |
| 进程被强杀 | 下次启动 boot reconcile → 全部 `failed("interrupted by restart")` |
| 时钟回拨 | `nextRunAt` 已推进到未来，不会重复触发；回拨后可能推迟一次，可接受 |
| 自动化被删除时正在跑 | 软删不影响在飞运行；运行结算后写回已删记录（历史保留），
|  | 列表不显示 |

---

## 9. 测试计划

**Rust 单测**（`cron.rs` / `service.rs` / `store.rs` / `engine.rs` 内嵌 `#[cfg(test)]`）：

1. `normalize_cron`：5 字段通过与周字段重映射（`1`→周一、`7`→周日、
   `1-5` 不含周末）；6/7 字段拒绝；字段数错误报错。
2. `compute_next_run`：每分钟、每天 9 点、每周一 9 点跨 30 天窗口、
   2 月 30 日 → `None`、时区偏移生效（`Asia/Shanghai` 的 9 点 = UTC 1 点）。
3. `list_due` / `claim_due`：只有 enabled+schedule+到期被选中；第一次抢占成功
   并推进 `nextRunAt`，第二次抢占返回 None；跨多个错过的时点只补一次。
4. `start_run` / `settle_run` / `has_active_run` / `record_skipped_run`：
   状态与父行 `lastRunStatus` / `unseenFailures` 的写回。
5. `boot_reconcile_interrupted`：所有 running → failed + 固定错误串。
6. `prune_old_runs`：超期记录被删、未超期保留。
7. `mark_all_seen`：清零全部 `unseenFailures`。
8. `store`：缺文件 / 损坏（`.bak` 写于覆盖**之前**，用顺序断言）/ 未知键保留 /
   读写往返。
9. 引擎锁：同目录第二次 `acquire` 返回 `Taken`；释放后可再取。
10. `enqueue_task` 分支（不打真引擎）：建出的任务字段与自动化一致、
    运行结算为 `succeeded` 且 `taskId` 正确。

**前端**：`scheduleLabel.ts` 纯函数单测（若有 vitest 环境；否则靠
`vue-tsc` + 手工走查），mock 通道手测列表/编辑/启停/运行历史。

**验证命令**：`cargo test --manifest-path src-tauri/Cargo.toml`
与 `pnpm build`（含 `vue-tsc --noEmit`）。

---

## 10. 风险与回滚

| 风险 | 缓解 |
|---|---|
| 无头会话自动允许权限，agent 可能执行破坏性命令 | 编辑器显式提示；自动化创建时二次确认；只在该 conn 上生效 |
| 自实现 cron 的 DST 语义偏差 | 明显标注 + 表外时区按 UTC；主要用户无 DST |
| `automation/*` 不阻塞 UI 主线程 | 所有 git/进程操作在 `spawn_blocking` / `tokio::spawn` |
| 双开桌面导致两个调度器 | 独占文件锁 fail-closed |
| 新增依赖 | 不新增（cron 自实现、无 chrono） |

**回滚**：功能整体是**新增模块 + 一个新页面 + 一个侧栏项**，
不修改 `tasks.json` / `acp.rs` 既有语义（只把 `task_create` 的函数体抽成
内部函数、给 `AcpConn` 加一个 `owner` 字段默认 `User`）。
回滚 = 删掉 `automation/` 模块、取消 `lib.rs` 注册、移除路由与侧栏项；
`automations.json` 留在磁盘上无害。
