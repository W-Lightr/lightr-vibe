//! 通用子代理委派控制面。
//!
//! ACP 只负责单个 Agent 会话；本模块保存父子关系、验证委派请求并驱动
//! 独立的子 ACP 会话。所有持久化均限定在应用自己的 namespace 下。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

use crate::agent_type::AgentType;

const MAX_DEPTH: u32 = 4;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegationStatus {
    Running,
    WaitingPermission,
    WaitingQuestion,
    Completed,
    Failed,
    Canceled,
    Interrupted,
}

impl DelegationStatus {
    fn terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Canceled | Self::Interrupted)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegationRecord {
    pub task_id: String,
    pub parent_session_id: String,
    pub child_session_id: String,
    pub agent_type: String,
    pub task: String,
    pub cwd: String,
    pub status: DelegationStatus,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegationSettings {
    pub enabled: bool,
    pub depth_limit: u32,
}

impl Default for DelegationSettings {
    fn default() -> Self { Self { enabled: false, depth_limit: 1 } }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegationStartedEvent {
    pub record: DelegationRecord,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn app_dir() -> Result<PathBuf, String> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or("无法定位用户目录")?;
    Ok(home.join(".lightr-vibe").join("lightr-vibe"))
}

fn records_path() -> Result<PathBuf, String> { Ok(app_dir()?.join("delegations.json")) }
fn settings_path() -> Result<PathBuf, String> { Ok(app_dir()?.join("delegation-settings.json")) }

fn read_json<T: for<'a> Deserialize<'a>>(path: &Path) -> Option<T> {
    fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok())
}

fn atomic_write(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let parent = path.parent().ok_or("委派数据路径无父目录")?;
    fs::create_dir_all(parent).map_err(|e| format!("创建委派数据目录失败: {e}"))?;
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, text).map_err(|e| format!("写入委派数据失败: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| { let _ = fs::remove_file(&tmp); format!("保存委派数据失败: {e}") })
}

fn settings_inner() -> DelegationSettings {
    read_json(&settings_path().ok().unwrap_or_default()).unwrap_or_default()
}

fn records() -> &'static Mutex<HashMap<String, DelegationRecord>> {
    static RECORDS: OnceLock<Mutex<HashMap<String, DelegationRecord>>> = OnceLock::new();
    RECORDS.get_or_init(|| {
        let mut restored: HashMap<String, DelegationRecord> = read_json(&records_path().ok().unwrap_or_default()).unwrap_or_default();
        let now = now_ms();
        let mut changed = false;
        for record in restored.values_mut() {
            if !record.status.terminal() {
                record.status = DelegationStatus::Interrupted;
                record.error_code = Some("app_restarted".into());
                record.message = Some("应用重启，未完成的子代理已中断".into());
                record.updated_at = now;
                changed = true;
            }
        }
        if changed { let _ = persist_records(&restored); }
        Mutex::new(restored)
    })
}

fn persist_records(records: &HashMap<String, DelegationRecord>) -> Result<(), String> {
    atomic_write(&records_path()?, records)
}

fn normalize(raw: &str) -> PathBuf {
    let mut result = PathBuf::new();
    for component in Path::new(raw).components() {
        match component {
            Component::CurDir => {},
            Component::ParentDir => { let _ = result.pop(); },
            other => result.push(other.as_os_str()),
        }
    }
    result
}

fn ensure_child_cwd(parent_cwd: &str, requested: Option<&str>) -> Result<String, String> {
    let parent = normalize(parent_cwd);
    let target = normalize(requested.unwrap_or(parent_cwd));
    if target.starts_with(&parent) { Ok(target.to_string_lossy().into_owned()) } else { Err("outside-project".into()) }
}

fn make_id(prefix: &str) -> String {
    static NEXT: OnceLock<std::sync::atomic::AtomicU64> = OnceLock::new();
    let next = NEXT.get_or_init(|| std::sync::atomic::AtomicU64::new(1)).fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    format!("{prefix}-{:x}{:x}", now_ms(), next)
}

pub fn get_settings() -> DelegationSettings { settings_inner() }

#[tauri::command]
pub fn delegation_get_settings() -> DelegationSettings { get_settings() }

#[tauri::command]
pub fn delegation_set_settings(settings: DelegationSettings) -> Result<DelegationSettings, String> {
    let saved = DelegationSettings { enabled: settings.enabled, depth_limit: settings.depth_limit.clamp(1, MAX_DEPTH) };
    atomic_write(&settings_path()?, &saved)?;
    Ok(saved)
}

#[tauri::command]
pub fn delegation_list() -> Vec<DelegationRecord> {
    let mut items: Vec<_> = records().lock().unwrap().values().cloned().collect();
    items.sort_by_key(|record| std::cmp::Reverse(record.updated_at));
    items
}

pub fn status_for_parent(parent: &str, task_ids: &[String]) -> Vec<DelegationRecord> {
    let map = records().lock().unwrap();
    task_ids.iter().filter_map(|id| map.get(id).filter(|r| r.parent_session_id == parent).cloned()).collect()
}

pub fn delegate(
    app: AppHandle,
    parent_session_id: String,
    parent_cwd: String,
    agent_type: String,
    task: String,
    requested_cwd: Option<String>,
) -> Result<DelegationRecord, String> {
    let settings = get_settings();
    if !settings.enabled { return Err("delegation-disabled".into()); }
    let engine = AgentType::from_wire(&agent_type).ok_or("invalid-agent-type")?;
    if task.trim().is_empty() { return Err("任务不能为空".into()); }
    let cwd = ensure_child_cwd(&parent_cwd, requested_cwd.as_deref())?;
    // 沿 parent -> child 关系向上计算真实层级。默认 1 表示只有顶层
    // 用户会话可直接委派，子代理不能继续递归委派。
    let map = records().lock().unwrap();
    let mut depth = 0;
    let mut cursor = parent_session_id.as_str();
    while let Some(parent) = map.values().find(|r| r.child_session_id == cursor) {
        depth += 1;
        cursor = parent.parent_session_id.as_str();
    }
    drop(map);
    if depth >= settings.depth_limit { return Err("depth-limit".into()); }
    let now = now_ms();
    let record = DelegationRecord {
        task_id: make_id("dgt"), child_session_id: make_id("ses"), parent_session_id,
        agent_type, task: task.trim().to_string(), cwd, status: DelegationStatus::Running,
        created_at: now, updated_at: now, error_code: None, message: Some("子代理正在启动".into()), summary: None,
    };
    {
        let mut map = records().lock().unwrap();
        map.insert(record.task_id.clone(), record.clone());
        persist_records(&map)?;
    }
    let _ = app.emit("delegation://started", DelegationStartedEvent { record: record.clone() });
    let task_id = record.task_id.clone();
    let child_id = record.child_session_id.clone();
    let cwd = record.cwd.clone();
    let text = record.task.clone();
    std::thread::spawn(move || crate::acp::run_delegated_child(app, task_id, child_id, engine, cwd, text));
    Ok(record)
}

pub fn set_status(app: &AppHandle, task_id: &str, status: DelegationStatus, message: Option<String>, summary: Option<String>, error_code: Option<String>) {
    let changed = {
        let mut map = records().lock().unwrap();
        let Some(record) = map.get_mut(task_id) else { return; };
        // cancel/disconnect 与 prompt 返回可能并发；终态不得被迟到结果覆盖。
        if record.status.terminal() { return; }
        record.status = status;
        record.updated_at = now_ms();
        record.message = message;
        record.summary = summary;
        record.error_code = error_code;
        let cloned = record.clone();
        let _ = persist_records(&map);
        cloned
    };
    let _ = app.emit("delegation://updated", changed);
}

/// ACP 流式摘要专用：只把仍未结束的 child 当作自动化连接处理。
pub fn is_active_child(child_session_id: &str) -> bool {
    records().lock().unwrap().values().any(|record| {
        record.child_session_id == child_session_id && !record.status.terminal()
    })
}

pub fn mark_child_waiting(app: &AppHandle, child_session_id: &str, question: bool) {
    let id = records().lock().unwrap().values().find(|r| r.child_session_id == child_session_id && !r.status.terminal()).map(|r| r.task_id.clone());
    if let Some(id) = id {
        set_status(app, &id, if question { DelegationStatus::WaitingQuestion } else { DelegationStatus::WaitingPermission }, Some(if question { "等待用户回答".into() } else { "等待用户授权".into() }), None, None);
    }
}

/// 用户处理完 child 的权限/问题后恢复运行态；非等待态不发冗余事件。
pub fn mark_child_running(app: &AppHandle, child_session_id: &str) {
    let id = records().lock().unwrap().values().find(|record| {
        record.child_session_id == child_session_id
            && matches!(record.status, DelegationStatus::WaitingPermission | DelegationStatus::WaitingQuestion)
    }).map(|record| record.task_id.clone());
    if let Some(id) = id {
        set_status(app, &id, DelegationStatus::Running, Some("子代理继续运行".into()), None, None);
    }
}

pub fn cancel(app: &AppHandle, parent_session_id: &str, task_id: &str) -> Result<DelegationRecord, String> {
    let record = records().lock().unwrap().get(task_id).cloned().filter(|r| r.parent_session_id == parent_session_id).ok_or("unknown-task")?;
    if !record.status.terminal() {
        // 先持久化终态再取消底层 prompt，避免 RPC 返回的失败回调抢先覆盖为 failed。
        set_status(app, task_id, DelegationStatus::Canceled, Some("父会话取消了子代理".into()), None, Some("canceled".into()));
        crate::acp::cancel_delegated_child(app, &record.child_session_id);
    }
    Ok(records().lock().unwrap().get(task_id).cloned().unwrap_or(record))
}

pub fn cancel_children(app: &AppHandle, parent_session_id: &str) {
    let ids: Vec<String> = records().lock().unwrap().values().filter(|r| r.parent_session_id == parent_session_id && !r.status.terminal()).map(|r| r.task_id.clone()).collect();
    for id in ids { let _ = cancel(app, parent_session_id, &id); }
}

#[cfg(test)]
mod tests {
    use super::{ensure_child_cwd, DelegationStatus};

    #[test]
    fn child_cwd_must_stay_below_parent() {
        assert_eq!(ensure_child_cwd("C:/workspace/app", Some("C:/workspace/app/packages/ui")).unwrap(), "C:\\workspace\\app\\packages\\ui");
        assert_eq!(ensure_child_cwd("C:/workspace/app", None).unwrap(), "C:\\workspace\\app");
        assert_eq!(ensure_child_cwd("C:/workspace/app", Some("C:/workspace/other")).unwrap_err(), "outside-project");
    }

    #[test]
    fn only_final_states_are_terminal() {
        assert!(!DelegationStatus::Running.terminal());
        assert!(!DelegationStatus::WaitingPermission.terminal());
        assert!(DelegationStatus::Completed.terminal());
        assert!(DelegationStatus::Canceled.terminal());
    }
}
