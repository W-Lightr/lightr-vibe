// 调度引擎（design §5）：独占锁证明唯一引擎 → boot 恢复 → 主循环。
//
// 主循环三条 tick：
// - 30s 调度：list_due → CAS claim_due → spawn(run_automation)
// - 30s 对账：本进程没在跟踪、且超 180 分钟的 running → failed
// - 6h  剪枝：删除 30 天前的运行记录
//
// 三条不变量（design §2）：
// 1. CAS 抢占时间片（service::claim_due）—— 多进程/重启不漏不重
// 2. fire 锁串行化「查重 + 插入 + 启动」 —— 手动与定时撞车不会起两条
// 3. 独占文件锁证明唯一引擎 —— 才敢在启动时把 running 一律判死

use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use tauri::async_runtime::Mutex;
use tauri::{AppHandle, Emitter};

use crate::acp;
use crate::agent_type::AgentType;
use crate::automation::service::{self, RunFacts};
use crate::automation::store::{
    self, now_ms, Automation, AutomationAction, AutomationRunStatus,
};

/// 调度 / 对账轮询间隔（cron 是分钟粒度，30s 足以抓住每个时点）
const SCHEDULER_INTERVAL_SECS: u64 = 30;
const RECONCILE_INTERVAL_SECS: u64 = 30;
/// 运行历史剪枝间隔
const PRUNE_INTERVAL_SECS: u64 = 6 * 60 * 60;
/// 本进程不再跟踪的运行，超过这个时长强制失败
const MAX_RUN_MINUTES: i64 = 180;

/// 前端监听的事件名：payload 只带 `{ id, runId? }`，收到就重读整表
pub const AUTOMATION_CHANGED_EVENT: &str = "automation://changed";

static ENGINE: OnceLock<Arc<AutomationEngine>> = OnceLock::new();

/// 进程全局引擎句柄。抢不到独占锁时永远为 None。
pub fn engine() -> Option<Arc<AutomationEngine>> {
    ENGINE.get().cloned()
}

/// 一次在飞运行的跟踪信息
#[derive(Debug, Clone)]
struct InFlight {
    /// 本进程开始跟踪它的时刻（UTC 毫秒）。用于对账时判断「这轮跟了多久」——
    /// 运行记录里的 started_at 是写库时间，可能早于真正开始跟踪。
    started_at: i64,
}

impl InFlight {
    fn elapsed_ms(&self, now: i64) -> i64 {
        now.saturating_sub(self.started_at)
    }
}

pub struct AutomationEngine {
    app: AppHandle,
    /// 每条自动化一把 fire 锁：串行化「查重 + 插入 + 启动」
    fire_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    /// 本进程正在跟踪的运行
    in_flight: Arc<Mutex<HashMap<String, InFlight>>>,
    /// 保活：独占锁的文件句柄。Drop 时 OS 释放锁，下一次启动才能正确恢复。
    _engine_lock: File,
}

/// 尝试成为唯一引擎的结果
enum Ownership {
    Exclusive(File),
    /// 另一个活进程持有锁
    Taken,
    /// 锁文件根本打不开（真实 IO 错误）—— fail closed
    Unavailable,
}

fn engine_lock_path() -> Result<PathBuf, String> {
    let path = store::automations_file_path()?;
    let dir = path
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "io: 自动化数据目录不合法".to_string())?;
    Ok(dir.join("automations.json.lock"))
}

/// 拿独占 advisory 锁（跨平台：Unix flock / Windows LockFileEx）。
///
/// 句柄必须活到进程退出：它既是「我是唯一引擎」的证明，也是下次启动能正确
/// 恢复的前提。锁文件是纯句柄，不写内容、不 truncate。
fn acquire_engine_ownership() -> Ownership {
    let path = match engine_lock_path() {
        Ok(p) => p,
        Err(_) => return Ownership::Unavailable,
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let file = match std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[automation] 打开引擎锁失败: {}", e);
            return Ownership::Unavailable;
        }
    };
    match file.try_lock() {
        Ok(()) => Ownership::Exclusive(file),
        Err(std::fs::TryLockError::WouldBlock) => Ownership::Taken,
        Err(std::fs::TryLockError::Error(e)) => {
            eprintln!("[automation] 获取引擎锁失败: {}", e);
            Ownership::Unavailable
        }
    }
}

/// 启动引擎：拿锁 → boot 恢复 → 建全局句柄 → spawn 主循环。
///
/// **顺序关键**：先拿锁再判死。若先判死再拿锁，两个进程同时启动时后到的那个
/// 会把前者正在跑的记录误判死。
pub fn boot_engine(app: AppHandle) {
    let engine_lock = match acquire_engine_ownership() {
        Ownership::Exclusive(file) => file,
        Ownership::Taken => {
            eprintln!(
                "[automation] 另一个进程已持有自动化引擎锁，本进程不驱动自动化\
                 （CRUD 仍可用，手动运行会返回 engine not running）"
            );
            return;
        }
        Ownership::Unavailable => {
            eprintln!("[automation] 无法建立引擎锁，本进程禁用自动化引擎");
            return;
        }
    };

    // 走到这里可以证明本进程是唯一引擎，激进恢复才是安全的
    match service::boot_reconcile_interrupted() {
        Ok(n) if n > 0 => {
            eprintln!("[automation] 启动恢复：{} 条运行被判为中断失败", n);
        }
        Ok(_) => {}
        Err(e) => eprintln!("[automation] 启动恢复失败: {}", e),
    }

    let engine = Arc::new(AutomationEngine {
        app,
        fire_locks: Arc::new(Mutex::new(HashMap::new())),
        in_flight: Arc::new(Mutex::new(HashMap::new())),
        _engine_lock: engine_lock,
    });
    if ENGINE.set(engine.clone()).is_ok() {
        tauri::async_runtime::spawn(async move {
            run_automation_engine(engine).await;
        });
    }
}

/// 主循环（design 5.3）
///
/// tokio 不是本仓库的直接依赖（由 tauri 传递引入），所以这里用
/// `tauri::async_runtime` 的阻塞任务做三路轮询，而不是 select! + interval。
/// 代价是每个 tick 独立线程，但 tick 数量极少（30s / 6h），完全可接受。
pub async fn run_automation_engine(engine: Arc<AutomationEngine>) {
    let schedule_eng = engine.clone();
    tauri::async_runtime::spawn_blocking(move || loop {
        std::thread::sleep(Duration::from_secs(SCHEDULER_INTERVAL_SECS));
        schedule_tick(&schedule_eng);
    });

    let reconcile_eng = engine.clone();
    tauri::async_runtime::spawn_blocking(move || loop {
        std::thread::sleep(Duration::from_secs(RECONCILE_INTERVAL_SECS));
        reconcile_tick(&reconcile_eng);
    });

    let prune_eng = engine.clone();
    tauri::async_runtime::spawn_blocking(move || loop {
        std::thread::sleep(Duration::from_secs(PRUNE_INTERVAL_SECS));
        match service::prune_old_runs(service::RUN_RETENTION_DAYS) {
            Ok(n) if n > 0 => eprintln!("[automation] 剪枝：删除 {} 条旧运行记录", n),
            Ok(_) => {}
            Err(e) => eprintln!("[automation] 剪枝失败: {}", e),
        }
        let _ = &prune_eng;
    });

    // 启动后立刻补一次到期检查（design: 首次 tick 即时）
    schedule_tick(&engine);
    engine.reconcile_once().await;

    // 主任务保持存活（它持有 Arc，另外两路线程自己跑）
    std::future::pending::<()>().await;
}

/// 30s 调度：list_due -> CAS claim_due -> 后台触发
fn schedule_tick(engine: &Arc<AutomationEngine>) {
    let now = now_ms();
    let due = match service::list_due(now) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[automation] 列出到期自动化失败: {}", e);
            return;
        }
    };
    for id in due {
        match service::claim_due(&id, now) {
            Ok(Some(slot)) => {
                let eng = engine.clone();
                // 启动可能要几秒（建进程），不能挡住后续 tick
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = eng.run_automation(&id, "schedule", Some(slot)).await {
                        eprintln!("[automation] 定时运行 {} 未成功: {}", id, e);
                    }
                });
            }
            Ok(None) => {}
            Err(e) => eprintln!("[automation] 抢占 {} 失败: {}", id, e),
        }
    }
}

/// 30s 对账（同步版，供后台线程调用）
fn reconcile_tick(engine: &Arc<AutomationEngine>) {
    if let Ok(handle) = tokio_handle() {
        handle.block_on(engine.reconcile_once());
    }
}

/// 借 tauri 的运行时句柄（不新增 tokio 依赖）
fn tokio_handle() -> Result<tauri::async_runtime::RuntimeHandle, String> {
    Ok(tauri::async_runtime::handle())
}

impl AutomationEngine {
    async fn fire_lock(&self, automation_id: &str) -> Arc<Mutex<()>> {
        let mut map = self.fire_locks.lock().await;
        map.entry(automation_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    fn emit_changed(&self, automation_id: &str, run_id: Option<&str>) {
        let _ = self.app.emit(
            AUTOMATION_CHANGED_EVENT,
            serde_json::json!({ "id": automation_id, "runId": run_id }),
        );
    }

    /// CRUD 命令用的广播（没有 run id）
    pub fn emit_changed_public(&self, automation_id: &str) {
        self.emit_changed(automation_id, None);
    }

    // ---------- 触发 ----------

    /// 触发一次运行（design §5.4）。
    ///
    /// 记录运行行 → 立刻广播 → 启动。**不等待完成**（`launch_session` 在
    /// `spawn_blocking` 里自己结算；`enqueue_task` 同步结算）。
    /// 启动前任何失败都必须结算成 `failed`，绝不允许静默挂住。
    pub async fn run_automation(
        &self,
        automation_id: &str,
        trigger: &str,
        scheduled_for: Option<i64>,
    ) -> Result<String, String> {
        // 串行化同一自动化的全部触发：否则「查重 + 插入」是 check-then-act 竞态，
        // 手动与定时撞车（或双击）会起两条
        let fire_lock = self.fire_lock(automation_id).await;
        let _guard = fire_lock.lock().await;

        let auto = service::get(automation_id)?;

        if service::has_active_run(automation_id)? {
            let _ = service::record_skipped_run(automation_id, trigger, scheduled_for);
            self.emit_changed(automation_id, None);
            return Err("previous run still active".to_string());
        }

        let run = service::start_run(automation_id, trigger, scheduled_for)?;
        // 立刻广播：launch 可能要几秒，UI 不必等
        self.emit_changed(automation_id, Some(&run.id));

        let result = match auto.action {
            AutomationAction::EnqueueTask => self.launch_enqueue_task(&auto, &run.id).await,
            AutomationAction::LaunchSession => self.launch_session(&auto, &run.id).await,
        };

        match result {
            Ok(()) => Ok(run.id),
            Err(e) => {
                let _ = service::settle_run(
                    &run.id,
                    AutomationRunStatus::Failed,
                    RunFacts {
                        error: Some(e.clone()),
                        ..Default::default()
                    },
                );
                self.emit_changed(automation_id, Some(&run.id));
                Err(e)
            }
        }
    }

    // ---------- 两条 launch 分支 ----------

    /// `enqueue_task`：建一张 todo 任务卡，同步结算成功（design §5.5 A）
    async fn launch_enqueue_task(&self, auto: &Automation, run_id: &str) -> Result<(), String> {
        ensure_project_dir(auto)?;

        let title: String = auto.name.chars().take(80).collect();
        let draft = crate::task::TaskDraft {
            project_id: auto.project_id.clone(),
            title: title.clone(),
            prompt: auto.prompt.clone(),
            engine_id: Some(auto.engine_id.clone()),
            model_id: auto.model_id.clone(),
            effort_id: auto.effort_id.clone(),
            effort_value: auto.effort_value.clone(),
            use_worktree: Some(auto.use_worktree),
            base_branch: auto.base_branch.clone(),
            init_command: auto.init_command.clone(),
        };
        // 走命令核心的同一个落盘函数：id / 事件 / 排序 / 原子写全部复用
        let details = crate::task::create_task_internal(draft)?;
        let task_id = details.task.id;

        let settled = service::settle_run(
            run_id,
            AutomationRunStatus::Succeeded,
            RunFacts {
                task_id: Some(task_id.clone()),
                stop_reason: Some("queued".to_string()),
                summary: Some(format!("queued task {}: {}", task_id, title)),
                ..Default::default()
            },
        )?;
        if settled.is_some() {
            self.emit_changed(&auto.id, Some(run_id));
        }
        Ok(())
    }

    /// `launch_session`：起一个无头 ACP 会话跑完一个 turn（design §5.5 B）
    async fn launch_session(&self, auto: &Automation, run_id: &str) -> Result<(), String> {
        ensure_project_dir(auto)?;
        if auto.prompt.trim().is_empty() {
            return Err("io: 提示词不能为空".to_string());
        }
        let agent = AgentType::from_wire(&auto.engine_id)
            .ok_or_else(|| format!("io: 未知引擎 {}", auto.engine_id))?;

        // 取消可能刚刚发生过：起进程前重读状态
        let current = service::get_run(run_id)?;
        if current.status.is_terminal() {
            return Ok(());
        }

        let conn_id = automation_conn_id(run_id);
        let mut inflight = self.in_flight.lock().await;
        inflight.insert(
            run_id.to_string(),
            InFlight {
                started_at: now_ms(),
            },
        );
        drop(inflight);

        let app = self.app.clone();
        let cwd = auto.root_path.clone();
        let model_id = auto.model_id.clone();
        let effort = match (auto.effort_id.clone(), auto.effort_value.clone()) {
            (Some(id), Some(value)) => Some((id, value)),
            _ => None,
        };
        let prompt = auto.prompt.clone();
        let conn_for_blocking = conn_id.clone();
        let run_id_for_turn = run_id.to_string();

        // turn 必须在后台跑：一轮可能几分钟，而 launch_session 是
        // `automation_run_now` 的调用栈 —— 同步 await 会让「立即运行」按钮挂到
        // 整轮结束才返回（UI 会以为请求没发出去）。准备阶段的错误仍然同步返回，
        // 之后的一切（结算 + 广播 + 断开）都在后台完成。
        let automation_id = auto.id.clone();
        let run_id_owned = run_id.to_string();
        // spawn 的 future 必须 'static，不能借用 &self
        let app_handle = self.app.clone();
        let in_flight = self.in_flight.clone();
        tauri::async_runtime::spawn(async move {
            let turn = tauri::async_runtime::spawn_blocking(move || {
                acp::run_automation_turn(
                    app.clone(),
                    conn_for_blocking.clone(),
                    run_id_for_turn.clone(),
                    agent,
                    cwd,
                    model_id,
                    effort,
                    prompt,
                )
            })
            .await;

            // 无论成败都断开：一个 turn 一个进程
            acp::shutdown_automation_conn(&app_handle, &conn_id);
            in_flight.lock().await.remove(&run_id_owned);

            let (status, facts) = match turn {
                Ok(Ok(outcome)) => {
                    let status = classify_stop_reason(&outcome.stop_reason);
                    let error = if matches!(status, AutomationRunStatus::Failed) {
                        Some(format!("agent stopped: {}", outcome.stop_reason))
                    } else {
                        None
                    };
                    (
                        status,
                        RunFacts {
                            session_id: Some(outcome.session_id),
                            stop_reason: Some(outcome.stop_reason),
                            error,
                            summary: outcome.summary,
                            ..Default::default()
                        },
                    )
                }
                Ok(Err(e)) => (
                    AutomationRunStatus::Failed,
                    RunFacts {
                        error: Some(e),
                        ..Default::default()
                    },
                ),
                Err(e) => (
                    AutomationRunStatus::Failed,
                    RunFacts {
                        error: Some(format!("io: 自动化任务执行失败: {}", e)),
                        ..Default::default()
                    },
                ),
            };

            if let Ok(Some(_)) = service::settle_run(&run_id_owned, status, facts) {
                let _ = app_handle.emit(
                    AUTOMATION_CHANGED_EVENT,
                    serde_json::json!({ "id": automation_id, "runId": run_id_owned }),
                );
            }
        });

        Ok(())
    }

    // ---------- 取消 ----------

    /// 取消一个在飞运行（design §5.6）
    ///
    /// 顺序：先结算（CAS）→ 立刻广播 → 才拆连接。这样对账不会把用户取消标成
    /// failed，UI 也不必等 teardown。
    pub async fn cancel_run(&self, run_id: &str) -> Result<(), String> {
        let run = service::get_run(run_id)?;
        let automation_id = run.automation_id.clone();

        let settled = service::settle_run(
            run_id,
            AutomationRunStatus::Cancelled,
            RunFacts {
                stop_reason: Some("cancelled".to_string()),
                ..Default::default()
            },
        )?;

        if settled.is_some() {
            self.emit_changed(&automation_id, Some(run_id));
        }

        // 与 launch 互斥：拆连接要么发生在 prompt 之前（launch 的终态检查会中止），
        // 要么发生在 turn 真在飞时（cancel 掉真 turn），不会穿插进 prompt 入队
        let fire_lock = self.fire_lock(&automation_id).await;
        let _guard = fire_lock.lock().await;

        // **无条件**按 run_id 推导 connId 去拆连接。
        //
        // 早先这里用 `if let Some(session_id) = run.session_id` 当门禁，是错的：
        // session_id 要等 turn 跑完由 settle_run 才写进运行记录，运行在飞时它是
        // None —— 于是取消只改了库里的状态，agent 进程继续在后台跑（泄进程，
        // 且 AC7「cancel 生效」实际不成立）。
        //
        // connId 是确定性的 `auto-<runId>`，不需要 session_id 来定位；连接不存在
        // 时 shutdown_automation_conn 内部是安全 no-op。
        let conn_id = automation_conn_id(run_id);
        acp::shutdown_automation_conn(&self.app, &conn_id);
        Ok(())
    }

    // ---------- 对账 ----------

    /// 对账（design §5.7）：本仓库的结算由 `run_automation` 自己完成，
    /// 所以这里只处理「本进程不再跟踪」的异常运行。
    async fn reconcile_once(&self) {
        let active = match service::list_active_runs() {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[automation] 对账读取失败: {}", e);
                return;
            }
        };
        if active.is_empty() {
            return;
        }
        let tracking: Vec<(String, i64)> = {
            let map = self.in_flight.lock().await;
            map.iter()
                .map(|(id, f)| (id.clone(), f.elapsed_ms(now_ms())))
                .collect()
        };
        let now = now_ms();

        for run in active {
            if let Some((_, tracked_ms)) = tracking.iter().find(|(id, _)| id == &run.id) {
                // 本进程在跟它，run_automation 会结算。超过 180 分钟还没结算说明
                // launch 自己卡住了，记一笔便于排查（但不主动干预）。
                if *tracked_ms > MAX_RUN_MINUTES * 60_000 {
                    eprintln!(
                        "[automation] run {} 已在飞 {} 分钟仍未结算",
                        run.id,
                        tracked_ms / 60_000
                    );
                }
                continue;
            }
            let started = run.started_at.unwrap_or(run.created_at);
            if now - started > MAX_RUN_MINUTES * 60_000 {
                if let Ok(Some(_)) = service::settle_run(
                    &run.id,
                    AutomationRunStatus::Failed,
                    RunFacts {
                        error: Some("run lost its worker".to_string()),
                        ..Default::default()
                    },
                ) {
                    self.emit_changed(&run.automation_id, Some(&run.id));
                }
            }
        }
    }
}

/// 项目目录必须存在（design §5.9）：不存在就失败，不重建、不猜。
fn ensure_project_dir(auto: &Automation) -> Result<(), String> {
    let path = Path::new(&auto.root_path);
    if auto.root_path.trim().is_empty() || !path.is_dir() {
        return Err(format!("io: 项目目录不存在: {}", auto.root_path));
    }
    Ok(())
}

/// `stopReason` → 运行状态（design §5.5 B）
fn classify_stop_reason(stop_reason: &str) -> AutomationRunStatus {
    match stop_reason {
        "end_turn" => AutomationRunStatus::Succeeded,
        "cancelled" => AutomationRunStatus::Cancelled,
        _ => AutomationRunStatus::Failed,
    }
}

/// 运行 -> connId。**确定性**：取消时必须重建同一个 id 才能找到那条连接。
fn automation_conn_id(run_id: &str) -> String {
    format!("auto-{}", run_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::store::{point_store_at, reset_store, test_guard};
    use std::path::PathBuf;

    #[test]
    fn classify_stop_reason_maps_outcomes() {
        assert_eq!(
            classify_stop_reason("end_turn"),
            AutomationRunStatus::Succeeded
        );
        assert_eq!(
            classify_stop_reason("cancelled"),
            AutomationRunStatus::Cancelled
        );
        assert_eq!(classify_stop_reason("refusal"), AutomationRunStatus::Failed);
        assert_eq!(classify_stop_reason("max_tokens"), AutomationRunStatus::Failed);
        assert_eq!(classify_stop_reason(""), AutomationRunStatus::Failed);
    }

    #[test]
    fn engine_lock_is_exclusive_and_released_on_drop() {
        let _g = test_guard();
        let path = temp_path("enginelock");
        point_store_at(&path);

        let held = match acquire_engine_ownership() {
            Ownership::Exclusive(f) => f,
            _other => panic!("第一次拿锁应成功"),
        };
        // 同一进程内第二次 try_lock 会 WouldBlock（std 的 flock 是 per-handle）
        assert!(
            matches!(acquire_engine_ownership(), Ownership::Taken),
            "第二次拿锁应被拒绝"
        );
        drop(held);
        assert!(
            matches!(acquire_engine_ownership(), Ownership::Exclusive(_)),
            "释放后应能重新拿锁"
        );
        reset_store();
    }

    fn temp_path(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("lightr-eng-{}-{}", tag, now_ms()));
        let _ = std::fs::create_dir_all(&root);
        root.join("automations.json")
    }

    #[test]
    fn enqueue_task_creates_a_todo_card_and_settles_succeeded() {
        let _g = test_guard();
        let path_auto = temp_path("enq");
        point_store_at(&path_auto);

        let task_root = std::env::temp_dir().join(format!("lightr-enq-tasks-{}", now_ms()));
        let _ = std::fs::create_dir_all(&task_root);
        let project_root = std::env::temp_dir().join(format!("lightr-enq-proj-{}", now_ms()));
        let _ = std::fs::create_dir_all(&project_root);
        // 任务 store 是进程级单例：必须用 task 测试的同一把 STORE_LOCK 与同一套
        // point/reset 助手，否则会与 task.rs 的测试交错执行、互相污染
        let _task_guard = crate::task::tests::lock_store();
        crate::task::tests::point_store_at(&task_root.join("tasks.json"));

        let draft = crate::automation::store::AutomationDraft {
            name: "每日巡检".into(),
            enabled: true,
            trigger_kind: crate::automation::store::TriggerKind::Manual,
            engine_id: "open_code".into(),
            project_id: "proj-a".into(),
            root_path: project_root.to_string_lossy().to_string(),
            prompt: "检查依赖".into(),
            action: AutomationAction::EnqueueTask,
            use_worktree: true,
            ..Default::default()
        };
        let auto = service::create(draft).unwrap();

        // 不经过 tauri / tokio：手写 launch_enqueue_task 的等价步骤（建卡 + 结算）
        let run = service::start_run(&auto.id, "manual", None).unwrap();
        let details = crate::task::create_task_internal(crate::task::TaskDraft {
            project_id: auto.project_id.clone(),
            title: auto.name.clone(),
            prompt: auto.prompt.clone(),
            engine_id: Some(auto.engine_id.clone()),
            model_id: auto.model_id.clone(),
            effort_id: auto.effort_id.clone(),
            effort_value: auto.effort_value.clone(),
            use_worktree: Some(auto.use_worktree),
            base_branch: auto.base_branch.clone(),
            init_command: auto.init_command.clone(),
        })
        .unwrap();
        let task_id = details.task.id.clone();
        service::settle_run(
            &run.id,
            AutomationRunStatus::Succeeded,
            RunFacts {
                task_id: Some(task_id.clone()),
                stop_reason: Some("queued".into()),
                summary: Some(format!("queued task {}: {}", task_id, auto.name)),
                ..Default::default()
            },
        )
        .unwrap();
        let run_status = service::list_runs(&auto.id, 5).unwrap()[0].status;
        assert_eq!(run_status, AutomationRunStatus::Succeeded);

        let tasks = crate::task::task_list(Some("proj-a".into())).unwrap();
        let created = tasks.iter().find(|t| t.id == task_id).unwrap();
        assert_eq!(created.title, "每日巡检");
        assert_eq!(created.prompt, "检查依赖");
        assert_eq!(created.status, crate::task::TaskStatus::Todo);
        assert!(created.use_worktree);

        crate::task::tests::reset_store();
        reset_store();
    }
}
