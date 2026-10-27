// 自动化命令面（design §6）：11 个命令，全部是薄壳。
//
// 错误串沿用仓库词汇：not-found / io: ... / automation engine not running。
// 每个写操作后广播 `automation://changed`（engine 里的 run_* 自己广播），
// 前端收到就重读整表（design §5.10）。

use serde::Serialize;

use super::engine;
use super::service;
use super::store::{
    Automation, AutomationDraft, AutomationRun, AutomationRunStatus, DEFAULT_RUN_LIMIT,
};
use super::cron;

/// engine 未运行时的固定错误串（前端据此区分「这台进程不驱动自动化」）
const ENGINE_NOT_RUNNING: &str = "automation engine not running";

// ---------- CRUD ----------

#[tauri::command]
pub fn automation_list() -> Result<Vec<Automation>, String> {
    service::list()
}

#[tauri::command]
pub fn automation_get(id: String) -> Result<Automation, String> {
    service::get(&id)
}

#[tauri::command]
pub fn automation_create(draft: AutomationDraft) -> Result<Automation, String> {
    let row = service::create(draft)?;
    broadcast(&row.id);
    Ok(row)
}

#[tauri::command]
pub fn automation_update(id: String, draft: AutomationDraft) -> Result<Automation, String> {
    let row = service::update(&id, draft)?;
    broadcast(&row.id);
    Ok(row)
}

#[tauri::command]
pub fn automation_set_enabled(id: String, enabled: bool) -> Result<Automation, String> {
    let row = service::set_enabled(&id, enabled)?;
    broadcast(&row.id);
    Ok(row)
}

#[tauri::command]
pub fn automation_delete(id: String) -> Result<(), String> {
    service::soft_delete(&id)?;
    broadcast(&id);
    Ok(())
}

/// 清零全部未读失败（进入自动化页时调用）
#[tauri::command]
pub fn automation_mark_seen() -> Result<(), String> {
    service::mark_all_seen()
}

// ---------- cron 预览 ----------

/// 编辑器「下次运行」预览。与调度器共用同一个 cron 求值器，
/// 所以预览的时刻不可能与实际触发分叉（design §4.4）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NextRunPreview {
    pub next_run_at: Option<i64>,
    /// 该时区是否落在支持表内（表外按 UTC 求值，UI 应提示）
    pub timezone_supported: bool,
    /// 该时区是否有夏令时（本实现按标准时偏移求值，UI 应提示）
    pub timezone_has_dst: bool,
}

#[tauri::command]
pub fn automation_compute_next_run(cron: String, timezone: String) -> Result<NextRunPreview, String> {
    let next = cron::next_run_for(cron.trim(), timezone.trim(), super::store::now_ms())?;
    Ok(NextRunPreview {
        next_run_at: next,
        timezone_supported: cron::tz_supported(timezone.trim()),
        timezone_has_dst: cron::tz_has_dst(timezone.trim()),
    })
}

// ---------- 运行历史 ----------

#[tauri::command]
pub fn automation_runs(id: String, limit: Option<usize>) -> Result<Vec<AutomationRun>, String> {
    service::list_runs(&id, limit.unwrap_or(DEFAULT_RUN_LIMIT))
}

#[tauri::command]
pub fn automation_run_status(run_id: String) -> Result<AutomationRun, String> {
    service::get_run(&run_id)
}

// ---------- engine 驱动 ----------

/// 手动「立即运行」，返回新运行的 id
#[tauri::command]
pub async fn automation_run_now(id: String) -> Result<String, String> {
    let eng = engine::engine().ok_or_else(|| ENGINE_NOT_RUNNING.to_string())?;
    eng.run_automation(&id, "manual", None).await
}

/// 取消一个在飞运行
#[tauri::command]
pub async fn automation_cancel_run(run_id: String) -> Result<(), String> {
    let eng = engine::engine().ok_or_else(|| ENGINE_NOT_RUNNING.to_string())?;
    eng.cancel_run(&run_id).await
}

/// 运行状态枚举给前端做类型对齐用（避免 TS 重复定义一套字符串）
#[tauri::command]
pub fn automation_run_status_values() -> Vec<&'static str> {
    [
        AutomationRunStatus::Running,
        AutomationRunStatus::Succeeded,
        AutomationRunStatus::Failed,
        AutomationRunStatus::Cancelled,
        AutomationRunStatus::Skipped,
    ]
    .iter()
    .map(|s| s.as_str())
    .collect()
}

fn broadcast(automation_id: &str) {
    let Some(eng) = engine::engine() else {
        return;
    };
    eng.emit_changed_public(automation_id);
}
