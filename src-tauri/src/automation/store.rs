// automations.json 数据层（design §3）：单文件 JSON，全部经本模块读写。
//
// 约定（与 task.rs 同构）：
// - 文件 ~/.lightr-vibe/lightr-vibe/automations.json（app namespace 硬规则）
// - 单进程内 `AutomationStore.data` 一把 Mutex 串行化全部读写；写后整体落盘（原子写）
// - 损坏的 JSON 不覆盖：读失败回空数据，下次写入前把损坏原文备份成 automations.json.bak
// - 未知顶层字段在读写往返中保留（`#[serde(flatten)]`）
// - 时间统一 i64 UTC 毫秒（不引 chrono）
// - 错误串：not-found / io: ...

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// 文件格式版本
pub const FILE_VERSION: u32 = 1;

/// 运行历史的默认读取条数
pub const DEFAULT_RUN_LIMIT: usize = 20;

// ---------- 枚举 ----------

/// 触发方式：`schedule` 按 cron 跑；`manual` 只能手动「立即运行」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerKind {
    Schedule,
    Manual,
}

impl Default for TriggerKind {
    fn default() -> Self {
        TriggerKind::Manual
    }
}

/// 触发后做什么（存在自动化行里，不是运行记录）：
/// - `launch_session`：Rust 自己起一个无头 ACP 会话，跑完一个 turn 后结算
/// - `enqueue_task`：往任务看板塞一张 todo 卡，立刻结算成功
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationAction {
    LaunchSession,
    EnqueueTask,
}

impl Default for AutomationAction {
    fn default() -> Self {
        AutomationAction::LaunchSession
    }
}

/// 一次运行的生命周期。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationRunStatus {
    Running,
    Succeeded,
    Failed,
    Cancelled,
    /// 上一条还没跑完就又到点，被抑制掉的那次
    Skipped,
}

impl AutomationRunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            AutomationRunStatus::Running => "running",
            AutomationRunStatus::Succeeded => "succeeded",
            AutomationRunStatus::Failed => "failed",
            AutomationRunStatus::Cancelled => "cancelled",
            AutomationRunStatus::Skipped => "skipped",
        }
    }

    pub fn is_terminal(self) -> bool {
        !matches!(self, AutomationRunStatus::Running)
    }
}

// ---------- 实体 ----------

/// 一条自动化（design §3.1）。所有字段 `#[serde(default)]`，旧文件可直接读。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Automation {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub trigger_kind: TriggerKind,
    /// 5 字段 cron（分 时 日 月 周），UI 语义：周字段 0/7=周日
    pub cron: String,
    /// IANA 时区名（如 "Asia/Shanghai"）
    pub timezone: String,
    /// 下次触发时刻（UTC 毫秒）；None = 不再触发
    pub next_run_at: Option<i64>,
    pub engine_id: String,
    pub project_id: String,
    /// 项目根目录快照（触发前校验目录存在，不重建不猜）
    pub root_path: String,
    pub model_id: Option<String>,
    pub effort_id: Option<String>,
    pub effort_value: Option<String>,
    pub prompt: String,
    pub action: AutomationAction,
    /// 建出的任务是否使用工作树（透传给任务的 useWorktree）
    pub use_worktree: bool,
    pub base_branch: Option<String>,
    pub init_command: Option<String>,
    pub last_run_at: Option<i64>,
    pub last_run_status: Option<String>,
    pub last_run_id: Option<String>,
    /// 用户还没看过的失败次数（侧栏徽标）
    pub unseen_failures: i32,
    pub created_at: i64,
    pub updated_at: i64,
    /// 软删：列表过滤，运行历史保留
    pub deleted_at: Option<i64>,
}

impl Default for Automation {
    fn default() -> Self {
        Automation {
            id: String::new(),
            name: String::new(),
            enabled: false,
            trigger_kind: TriggerKind::default(),
            cron: String::new(),
            timezone: "Asia/Shanghai".to_string(),
            next_run_at: None,
            engine_id: String::new(),
            project_id: String::new(),
            root_path: String::new(),
            model_id: None,
            effort_id: None,
            effort_value: None,
            prompt: String::new(),
            action: AutomationAction::default(),
            use_worktree: true,
            base_branch: None,
            init_command: None,
            last_run_at: None,
            last_run_status: None,
            last_run_id: None,
            unseen_failures: 0,
            created_at: 0,
            updated_at: 0,
            deleted_at: None,
        }
    }
}

/// 新建 / 编辑的完整草稿（编辑器整条保存，无局部补丁语义）
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AutomationDraft {
    pub name: String,
    pub enabled: bool,
    pub trigger_kind: TriggerKind,
    pub cron: String,
    pub timezone: String,
    pub engine_id: String,
    pub project_id: String,
    pub root_path: String,
    pub model_id: Option<String>,
    pub effort_id: Option<String>,
    pub effort_value: Option<String>,
    pub prompt: String,
    pub action: AutomationAction,
    pub use_worktree: bool,
    pub base_branch: Option<String>,
    pub init_command: Option<String>,
}

impl Default for AutomationDraft {
    fn default() -> Self {
        AutomationDraft {
            name: String::new(),
            enabled: false,
            trigger_kind: TriggerKind::default(),
            cron: String::new(),
            timezone: "Asia/Shanghai".to_string(),
            engine_id: String::new(),
            project_id: String::new(),
            root_path: String::new(),
            model_id: None,
            effort_id: None,
            effort_value: None,
            prompt: String::new(),
            action: AutomationAction::default(),
            use_worktree: true,
            base_branch: None,
            init_command: None,
        }
    }
}

/// 一条运行记录（design §3.1）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AutomationRun {
    pub id: String,
    pub automation_id: String,
    pub status: AutomationRunStatus,
    /// "schedule" | "manual"
    pub trigger: String,
    /// 本次消费的时间片（UTC 毫秒）；manual 为 None
    pub scheduled_for: Option<i64>,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    /// launch_session 运行时的会话 id
    pub session_id: Option<String>,
    /// enqueue_task 建出的任务 id
    pub task_id: Option<String>,
    pub stop_reason: Option<String>,
    pub error: Option<String>,
    pub summary: Option<String>,
    pub created_at: i64,
}

impl Default for AutomationRun {
    fn default() -> Self {
        AutomationRun {
            id: String::new(),
            automation_id: String::new(),
            status: AutomationRunStatus::Running,
            trigger: "manual".to_string(),
            scheduled_for: None,
            started_at: None,
            ended_at: None,
            session_id: None,
            task_id: None,
            stop_reason: None,
            error: None,
            summary: None,
            created_at: 0,
        }
    }
}

// ---------- 文件 ----------

/// automations.json 的结构
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AutomationFile {
    pub version: u32,
    pub automations: Vec<Automation>,
    pub runs: Vec<AutomationRun>,
    /// 版本兼容：未知顶层字段原样回写
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

// ---------- 存储 ----------

/// 内存中的文件状态：原始文本（用于损坏备份）+ 解析后数据 + 是否读取失败
struct AutomationData {
    raw: String,
    file: AutomationFile,
    corrupt: bool,
}

impl AutomationData {
    fn empty() -> Self {
        AutomationData {
            raw: String::new(),
            file: AutomationFile {
                version: FILE_VERSION,
                automations: Vec::new(),
                runs: Vec::new(),
                extra: BTreeMap::new(),
            },
            corrupt: false,
        }
    }

    fn committed(&mut self, text: String) {
        self.raw = text;
        self.corrupt = false;
    }
}

/// 全局单例：`path = None` 表示尚未解析路径；`data = None` 表示尚未加载
struct AutomationStore {
    path: Mutex<Option<PathBuf>>,
    data: Mutex<Option<AutomationData>>,
}

static AUTOMATION_STORE: OnceLock<AutomationStore> = OnceLock::new();
static PATH_WARNED: OnceLock<()> = OnceLock::new();

fn store() -> &'static AutomationStore {
    AUTOMATION_STORE.get_or_init(|| AutomationStore {
        path: Mutex::new(None),
        data: Mutex::new(None),
    })
}

fn home_dir() -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    let key = "USERPROFILE";
    #[cfg(not(target_os = "windows"))]
    let key = "HOME";
    std::env::var(key)
        .map(PathBuf::from)
        .map_err(|_| format!("io: 环境变量 {} 缺失", key))
}

fn app_data_dir() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".lightr-vibe").join("lightr-vibe"))
}

/// automations.json 路径。测试可用 `LIGHTR_AUTOMATION_TEST_PATH` 覆盖。
pub fn automations_file_path() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("LIGHTR_AUTOMATION_TEST_PATH") {
        if !p.trim().is_empty() {
            return Ok(PathBuf::from(p));
        }
    }
    Ok(app_data_dir()?.join("automations.json"))
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 生成本地唯一 id：`<prefix>-xxxxxxxx`
pub fn create_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mixed = (nanos as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ seq.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    format!("{}-{:08x}", prefix, (mixed & 0xFFFF_FFFF) as u32)
}

/// 原子写：临时文件 + rename，失败时清理临时文件
fn atomic_write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| format!("io: 创建目录失败: {}", e))?;
        }
    }
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "io: 目标文件名不合法".to_string())?;
    let tmp = path.with_file_name(format!("{}.lightr-tmp-{}", file_name, now_ms()));
    fs::write(&tmp, text).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("io: 写临时文件失败: {}", e)
    })?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("io: 替换文件失败: {}", e)
    })
}

/// 把损坏的原文备份成 automations.json.bak（**必须在覆盖之前**调用）
fn backup_corrupt(path: &Path, raw: &str) {
    let bak = path.with_file_name(format!(
        "{}.bak",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("automations.json")
    ));
    if let Err(e) = fs::write(&bak, raw) {
        eprintln!("[automation] 备份损坏的 automations.json 失败: {}", e);
    }
}

fn load_from_disk(path: &Path) -> AutomationData {
    let raw = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return AutomationData::empty(),
        Err(e) => {
            eprintln!("[automation] 读取 automations.json 失败: {}", e);
            return AutomationData::empty();
        }
    };
    if raw.trim().is_empty() {
        let mut data = AutomationData::empty();
        data.raw = raw;
        return data;
    }
    match serde_json::from_str::<AutomationFile>(&raw) {
        Ok(file) => AutomationData {
            raw,
            file,
            corrupt: false,
        },
        Err(e) => {
            eprintln!("[automation] automations.json 解析失败，下次写入前先备份原文: {}", e);
            AutomationData {
                raw,
                file: AutomationFile {
                    version: FILE_VERSION,
                    automations: Vec::new(),
                    runs: Vec::new(),
                    extra: BTreeMap::new(),
                },
                corrupt: true,
            }
        }
    }
}

fn ensure_loaded<'a>(
    path_guard: &'a mut Option<PathBuf>,
    data: &mut Option<AutomationData>,
) -> Result<&'a Path, String> {
    if path_guard.is_none() {
        match automations_file_path() {
            Ok(p) => {
                *data = Some(load_from_disk(&p));
                *path_guard = Some(p);
            }
            Err(e) => {
                if PATH_WARNED.set(()).is_ok() {
                    eprintln!("[automation] 无法解析用户目录，数据降级为内存: {}", e);
                }
                *data = Some(AutomationData::empty());
                return Ok(Path::new(""));
            }
        }
    }
    Ok(path_guard.as_deref().unwrap_or_else(|| Path::new("")))
}

/// 只读取：`f` 拿到快照，不落盘
pub fn with_read<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&AutomationFile) -> R,
{
    let st = store();
    let mut path_guard = st
        .path
        .lock()
        .map_err(|_| "io: automations.json 路径锁中断".to_string())?;
    let mut data = st
        .data
        .lock()
        .map_err(|_| "io: automations.json 数据锁中断".to_string())?;
    ensure_loaded(&mut path_guard, &mut data)?;
    let current = data
        .as_ref()
        .ok_or_else(|| "io: automations.json 数据不可用".to_string())?;
    Ok(f(&current.file))
}

/// 写事务：`f` 修改数据 → 序列化 → 原子写 → 更新快照。
/// 损坏备份发生在覆盖**之前**（与 task.rs 同一条顺序保证）。
pub fn with_write<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&mut AutomationFile) -> Result<R, String>,
{
    let st = store();
    let mut path_guard = st
        .path
        .lock()
        .map_err(|_| "io: automations.json 路径锁中断".to_string())?;
    let mut data = st
        .data
        .lock()
        .map_err(|_| "io: automations.json 数据锁中断".to_string())?;
    let path = ensure_loaded(&mut path_guard, &mut data)?.to_path_buf();
    let current = data
        .as_mut()
        .ok_or_else(|| "io: automations.json 数据不可用".to_string())?;
    let result = f(&mut current.file)?;
    if current.corrupt && !path.as_os_str().is_empty() {
        backup_corrupt(&path, &current.raw);
    }
    let text = save_serialized(&path, &mut current.file)?;
    current.committed(text);
    Ok(result)
}

fn save_serialized(path: &Path, file: &mut AutomationFile) -> Result<String, String> {
    file.version = FILE_VERSION;
    // 运行记录按创建时间升序落盘，便于人工查看
    file.runs.sort_by_key(|r| r.created_at);
    let text = serde_json::to_string_pretty(file)
        .map_err(|e| format!("io: 序列化自动化数据失败: {}", e))?;
    if path.as_os_str().is_empty() {
        return Ok(text);
    }
    atomic_write(path, &text)?;
    Ok(text)
}

// ---------- 测试辅助 ----------

#[cfg(test)]
pub(crate) fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
pub(crate) fn point_store_at(path: &Path) {
    std::env::set_var("LIGHTR_AUTOMATION_TEST_PATH", path.as_os_str());
    *store().path.lock().unwrap() = None;
    *store().data.lock().unwrap() = None;
}

#[cfg(test)]
pub(crate) fn reset_store() {
    std::env::remove_var("LIGHTR_AUTOMATION_TEST_PATH");
    *store().path.lock().unwrap() = None;
    *store().data.lock().unwrap() = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("lightr-automation-test-{}-{}", tag, now_ms()));
        let _ = fs::create_dir_all(&root);
        root.join("automations.json")
    }

    #[test]
    fn missing_file_reads_empty_without_creating() {
        let _g = test_guard();
        let path = temp_file("missing");
        point_store_at(&path);
        let count = with_read(|f| f.automations.len()).unwrap();
        assert_eq!(count, 0);
        assert!(!path.exists(), "读取不应创建文件");
        reset_store();
    }

    #[test]
    fn corrupt_file_backs_up_before_overwrite() {
        let _g = test_guard();
        let path = temp_file("corrupt");
        fs::write(&path, "{ this is not json").unwrap();
        point_store_at(&path);
        // 读：退化为空，不抛错
        assert_eq!(with_read(|f| f.automations.len()).unwrap(), 0);
        // 写：先备份再覆盖
        with_write(|f| {
            f.automations.push(Automation::default());
            Ok(())
        })
        .unwrap();
        let bak = path.with_file_name("automations.json.bak");
        assert_eq!(fs::read_to_string(&bak).unwrap(), "{ this is not json");
        reset_store();
    }

    #[test]
    fn unknown_top_level_key_survives_roundtrip() {
        let _g = test_guard();
        let path = temp_file("extra");
        fs::write(
            &path,
            r#"{"version":1,"automations":[],"runs":[],"futureFlag":{"a":1}}"#,
        )
        .unwrap();
        point_store_at(&path);
        with_write(|f| {
            f.automations.push(Automation::default());
            Ok(())
        })
        .unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("futureFlag"), "未知顶层键应保留: {}", text);
        reset_store();
    }

    #[test]
    fn write_then_read_roundtrip() {
        let _g = test_guard();
        let path = temp_file("roundtrip");
        point_store_at(&path);
        with_write(|f| {
            let mut a = Automation::default();
            a.id = "auto-1".into();
            a.name = "每早巡检".into();
            a.trigger_kind = TriggerKind::Schedule;
            a.cron = "0 9 * * *".into();
            a.prompt = "检查依赖".into();
            f.automations.push(a);
            Ok(())
        })
        .unwrap();
        // 清空内存缓存，强制从磁盘重读（path 也要一起清，否则 ensure_loaded 不会重载 data）
        *store().data.lock().unwrap() = None;
        *store().path.lock().unwrap() = None;
        let name = with_read(|f| f.automations[0].name.clone()).unwrap();
        assert_eq!(name, "每早巡检");
        let kind = with_read(|f| f.automations[0].trigger_kind).unwrap();
        assert_eq!(kind, TriggerKind::Schedule);
        reset_store();
    }

    #[test]
    fn ids_are_unique_within_one_tick() {
        let a = create_id("auto");
        let b = create_id("auto");
        assert_ne!(a, b);
        assert!(a.starts_with("auto-"));
    }
}
