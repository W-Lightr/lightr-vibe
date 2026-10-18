// 任务看板数据层（task 10-17-task-data-layer）：任务 JSON 落盘、状态机校验、
// 排序 / 会话绑定 / 归档命令。设计见 tasks/10-17-task-board/design.md §2/§3。
//
// 约定：
// - 文件 ~/.lightr-vibe/lightr-vibe/tasks.json（命名空间硬规则，tauri-bridge spec）。
// - 单进程内 `TaskStore.data` 的 Mutex 串行化全部读写；写后整体落盘（原子写）。
// - 状态转移只在 Rust 校验（prd PAC3），前端 allowedDrop 仅是交互提示。
// - 错误串：not-found / stale / illegal-transition: <from>-><to> / io: ...
// - 损坏的 JSON 不覆盖：读失败回空数据，下次写入前把损坏文件备份为 tasks.json.bak。
// - 事件每任务保留最近 200 条（写入时截断）。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Emitter;

/// 每任务事件上限（design §2.2）
const MAX_EVENTS_PER_TASK: usize = 200;

/// 文件格式版本（design §2.2）
const FILE_VERSION: u32 = 1;

// ---------- 状态 ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskStatus {
    Todo,
    Queued,
    Preparing,
    Running,
    AwaitingInput,
    Review,
    /// 本任务不产生该状态，保留以兼容 codeg 列映射（design §3.1）
    Merging,
    Done,
    Failed,
    Canceled,
}

impl TaskStatus {
    /// wire 名（与前端 TaskStatus 联合类型逐字一致，也是错误串里的写法）
    pub fn as_wire(self) -> &'static str {
        match self {
            TaskStatus::Todo => "todo",
            TaskStatus::Queued => "queued",
            TaskStatus::Preparing => "preparing",
            TaskStatus::Running => "running",
            TaskStatus::AwaitingInput => "awaiting_input",
            TaskStatus::Review => "review",
            TaskStatus::Merging => "merging",
            TaskStatus::Done => "done",
            TaskStatus::Failed => "failed",
            TaskStatus::Canceled => "canceled",
        }
    }

    /// 全部状态（单测穷尽遍历；编译器会保证 match 覆盖所有变体）
    #[cfg(test)]
    pub const ALL: &'static [TaskStatus] = &[
        TaskStatus::Todo,
        TaskStatus::Queued,
        TaskStatus::Preparing,
        TaskStatus::Running,
        TaskStatus::AwaitingInput,
        TaskStatus::Review,
        TaskStatus::Merging,
        TaskStatus::Done,
        TaskStatus::Failed,
        TaskStatus::Canceled,
    ];

    /// 终态：可归档（prd R3 task_archive）
    fn is_terminal(self) -> bool {
        matches!(
            self,
            TaskStatus::Done | TaskStatus::Canceled | TaskStatus::Failed
        )
    }
}

/// 状态转换表（唯一权威，design §3.1）。`merging` 无出边、`done` 仅归档。
pub const TRANSITIONS: &[(TaskStatus, &[TaskStatus])] = &[
    (TaskStatus::Todo, &[TaskStatus::Queued, TaskStatus::Canceled]),
    (
        TaskStatus::Queued,
        &[TaskStatus::Preparing, TaskStatus::Todo, TaskStatus::Canceled],
    ),
    (
        TaskStatus::Preparing,
        &[
            TaskStatus::Running,
            TaskStatus::Failed,
            TaskStatus::Canceled,
        ],
    ),
    (
        TaskStatus::Running,
        &[
            TaskStatus::AwaitingInput,
            TaskStatus::Review,
            TaskStatus::Failed,
            TaskStatus::Canceled,
        ],
    ),
    (
        TaskStatus::AwaitingInput,
        &[TaskStatus::Running, TaskStatus::Canceled],
    ),
    (
        TaskStatus::Review,
        &[TaskStatus::Done, TaskStatus::Todo, TaskStatus::Canceled],
    ),
    (
        TaskStatus::Failed,
        &[TaskStatus::Queued, TaskStatus::Canceled],
    ),
    (TaskStatus::Canceled, &[TaskStatus::Todo]),
    (TaskStatus::Merging, &[]),
    (TaskStatus::Done, &[]),
];

/// 纯函数：from 是否允许转移到 to（含自环，UI 同列重排用）
pub fn can_transition(from: TaskStatus, to: TaskStatus) -> bool {
    if from == to {
        return true;
    }
    TRANSITIONS
        .iter()
        .find(|(status, _)| *status == from)
        .map(|(_, targets)| targets.contains(&to))
        .unwrap_or(false)
}

// ---------- 数据模型（camelCase 与前端 types/task.ts 逐字对齐） ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskVerdict {
    Success,
    NeedsReview,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskFailureReason {
    AgentError,
    Interrupted,
    EmptyTurn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkTask {
    pub id: String,
    /// `Project.id`（`proj-xxxx`）；项目被移除后仍保留（prd PAC4）
    pub project_id: String,
    pub title: String,
    /// 发给 Agent 的正文
    pub prompt: String,
    pub status: TaskStatus,
    /// 列内排序，仅同 projectId 内有意义
    pub sort_order: i64,
    /// 每次启动 +1；用于丢弃迟到的 turn 回调
    pub run_seq: u64,
    /// 一对一 ACP 会话
    pub session_id: Option<String>,
    /// 创建时固化的引擎 wire 标识
    pub engine_id: Option<String>,
    pub verdict: Option<TaskVerdict>,
    pub result_summary: Option<String>,
    pub last_error: Option<String>,
    pub failure_reason: Option<TaskFailureReason>,
    pub archived_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub started_at: Option<i64>,
    /// 进入 review / failed
    pub settled_at: Option<i64>,
    /// 进入 done / canceled
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskEventKind {
    Created,
    StatusChanged,
    UserAction,
    AgentProgress,
    AgentVerdict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskActor {
    User,
    Engine,
    Agent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkTaskEvent {
    pub id: String,
    pub task_id: String,
    pub kind: TaskEventKind,
    pub actor: TaskActor,
    pub payload: Option<serde_json::Value>,
    pub created_at: i64,
}

/// 新建任务入参（prd R3 task_create）
/// `Deserialize` 供 `task_create` 使用；`Serialize` 供 `task_duplicate` 入参直接复用
/// （复制 = 把源行改标题/提示词后重新创建）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDraft {
    pub project_id: String,
    pub title: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub engine_id: Option<String>,
}

/// 编辑入参（prd R3 task_update）：未出现的字段不改
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPatch {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub engine_id: Option<String>,
}

/// 一个任务的完整视图：行 + 时间线（设计 D4 详情抽屉一次取全）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDetails {
    pub task: WorkTask,
    pub events: Vec<WorkTaskEvent>,
}

/// turn 回流结论（task_settle_turn，子任务 3 / prd R5）。
/// 由前端从 `acp_prompt` 的 stopReason + 错误分类得出；Rust 侧只负责
/// 「runSeq 防串 + 状态机校验 + 写 failureReason」，不解释 agent 协议。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskTurnOutcome {
    /// 正常结束 → review
    Ok,
    /// agent 报错 → failed / agent_error
    Error,
    /// 空回复 → failed / empty_turn
    Empty,
    /// 用户主动停止 → 不做任何状态变更（任务保持 running，交给用户打回/通过）
    Cancelled,
}

// ---------- 文件格式 ----------

/// tasks.json：`{ version, tasks, events }`（events 按任务归档，避免 O(n) 全表扫描）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TaskFile {
    version: u32,
    tasks: Vec<WorkTask>,
    events: Vec<WorkTaskEvent>,
    /// 单任务事件保留上限（创建时从常量写入；容忍外部配置）
    #[serde(default = "default_max_events")]
    max_events_per_task: usize,
    /// 将来版本新增的未知顶层字段（前后端都不认识时原样带回，design §2.2）
    #[serde(flatten, default)]
    extra: BTreeMap<String, serde_json::Value>,
}

fn default_max_events() -> usize {
    MAX_EVENTS_PER_TASK
}

// ---------- 存储 ----------

/// 内存中的任务文件状态：原始文本（磁盘现状）+ 解析结果 + 是否为损坏文本。
/// `corrupt = true` 表示读入时原文无法解析，下次写回前先备份（R2）。
struct TaskData {
    raw: String,
    file: TaskFile,
    corrupt: bool,
}

impl TaskData {
    /// 空文件（磁盘不存在或首次加载）
    fn empty() -> Self {
        TaskData {
            raw: String::new(),
            file: TaskFile::default(),
            corrupt: false,
        }
    }

    /// 写回成功后同步快照（`corrupt` 置 false，避免重复备份）
    fn committed(&mut self, text: String) {
        self.raw = text;
        self.corrupt = false;
    }
}

/// tasks.json 的进程内单例：path 在首次访问时解析，data 由 Mutex 串行化。
/// `data = None` 表示尚未从磁盘加载；`path = None` 表示未解析到路径（降级为纯内存）。
struct TaskStore {
    path: Mutex<Option<PathBuf>>,
    data: Mutex<Option<TaskData>>,
}

static TASK_STORE: OnceLock<TaskStore> = OnceLock::new();
/// 文件路径解析失败时是否已记录过日志（避免每个命令刷一次）
static PATH_WARNED: OnceLock<()> = OnceLock::new();

/// 测试专用路径覆盖：单测把命令链路指向临时目录，绝不碰真实 tasks.json。
#[cfg(test)]
static TEST_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

fn store() -> &'static TaskStore {
    TASK_STORE.get_or_init(|| TaskStore {
        path: Mutex::new(None),
        data: Mutex::new(None),
    })
}

fn home_dir() -> Result<PathBuf, String> {
    // why: 不引入 dirs crate；Windows 用 USERPROFILE、类 Unix 用 HOME 足够
    #[cfg(target_os = "windows")]
    let key = "USERPROFILE";
    #[cfg(not(target_os = "windows"))]
    let key = "HOME";
    std::env::var(key)
        .map(PathBuf::from)
        .map_err(|_| format!("环境变量 {} 缺失", key))
}

fn tasks_file_path() -> Result<PathBuf, String> {
    #[cfg(test)]
    if let Some(p) = TEST_PATH.lock().ok().and_then(|g| g.clone()) {
        return Ok(p);
    }
    Ok(home_dir()?
        .join(".lightr-vibe")
        .join("lightr-vibe")
        .join("tasks.json"))
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 命令用 ID：`task-xxxxxxxx`（同前端 utils/id.ts 的前缀形状）
fn create_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // 进程内计数器保证同一时刻多次创建不撞号
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mixed = (nanos as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ seq.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    format!("{}-{:08x}", prefix, (mixed & 0xFFFF_FFFF) as u32)
}

/// 原子写（同 fs.rs 语义）：临时文件 + rename，失败清理临时文件。
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
        format!("io: 写入临时文件失败: {}", e)
    })?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("io: 重命名写入失败: {}", e)
    })
}

/// 备份损坏文件为 tasks.json.bak（覆盖旧备份；失败只记日志，不阻断写入）
fn backup_corrupt(path: &Path, raw: &str) {
    let bak = path.with_file_name(format!(
        "{}.bak",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("tasks.json")
    ));
    if let Err(e) = fs::write(&bak, raw) {
        eprintln!("[task] 备份损坏的 tasks.json 失败: {}", e);
    }
}

// 注意（设计 §2.2 前向兼容）：`TaskFile` 的 `extra` 用 `#[serde(flatten)]`，未知顶层字段
// 原样带回；已知字段的输出缩进不属契约，测试只断言内容。反序列化时 `tasks` / `events`
// 走 `Default`（缺省为空），损坏判定只看解析是否整体成功。

/// 加载磁盘内容：缺失 → 空数据（非错误）；损坏 → 空数据 + `corrupt: true`。
fn load_from_disk(path: &Path) -> TaskData {
    let raw = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return TaskData::empty(),
        Err(e) => {
            eprintln!("[task] 读取 tasks.json 失败: {}", e);
            return TaskData::empty();
        }
    };
    if raw.trim().is_empty() {
        return TaskData {
            raw,
            file: TaskFile::default(),
            corrupt: false,
        };
    }
    match serde_json::from_str::<TaskFile>(&raw) {
        Ok(file) => TaskData {
            raw,
            file,
            corrupt: false,
        },
        Err(e) => {
            // 不覆盖原文件：仅在下次写入前备份（R2）
            eprintln!("[task] tasks.json 解析失败，按空任务处理: {}", e);
            TaskData {
                raw,
                file: TaskFile::default(),
                corrupt: true,
            }
        }
    }
}

/// 惰性初始化：解析路径 + 加载磁盘内容（仅首次访问时执行一次）。
/// 返回 (磁盘路径, 是否已降级为纯内存)。路径缺失时 data 仍初始化为空，
/// 保证后续读写不会因 `None` 失败。
fn ensure_loaded<'a>(
    path_guard: &'a mut Option<PathBuf>,
    data: &mut Option<TaskData>,
) -> Result<&'a Path, String> {
    if path_guard.is_none() {
        match tasks_file_path() {
            Ok(p) => {
                *data = Some(load_from_disk(&p));
                *path_guard = Some(p);
            }
            Err(e) => {
                if PATH_WARNED.set(()).is_ok() {
                    eprintln!("[task] 无法解析用户目录，任务数据仅存内存: {}", e);
                }
                *data = Some(TaskData::empty());
                return Ok(Path::new(""));
            }
        }
    }
    Ok(path_guard.as_deref().unwrap_or_else(|| Path::new("")))
}

/// 读事务：不落盘
fn with_file_read<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&TaskData) -> R,
{
    let st = store();
    let mut path_guard = st
        .path
        .lock()
        .map_err(|_| "io: tasks.json 路径锁中毒".to_string())?;
    let mut data = st
        .data
        .lock()
        .map_err(|_| "io: tasks.json 数据锁中毒".to_string())?;
    ensure_loaded(&mut path_guard, &mut data)?;
    let current = data
        .as_ref()
        .ok_or_else(|| "io: tasks.json 数据不可用".to_string())?;
    Ok(f(current))
}

/// 写事务：`f` 修改数据 → 截断事件 → 序列化 → 原子写 → 更新快照。
/// 失败时内存态保留 `f` 的修改（与 fs 语义一致：调用方据错误决定后续）。
fn with_file_write<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&mut TaskFile) -> Result<R, String>,
{
    let st = store();
    let mut path_guard = st
        .path
        .lock()
        .map_err(|_| "io: tasks.json 路径锁中毒".to_string())?;
    let mut data = st
        .data
        .lock()
        .map_err(|_| "io: tasks.json 数据锁中毒".to_string())?;
    let path = ensure_loaded(&mut path_guard, &mut data)?.to_path_buf();
    let current = data
        .as_mut()
        .ok_or_else(|| "io: tasks.json 数据不可用".to_string())?;
    let result = f(&mut current.file)?;
    // 读入时损坏：**先**备份原文再写回（R2），首次写回后 corrupt 置 false。
    // 顺序不能反：备份必须在主文件被覆盖之前，否则备份到的是新内容。
    if current.corrupt && !path.as_os_str().is_empty() {
        backup_corrupt(&path, &current.raw);
    }
    let text = save_serialized(&path, &mut current.file)?;
    current.committed(text);
    Ok(result)
}

/// 落盘：更新版本 + 截断事件 + 序列化 + 原子写，返回写入文本
fn save_serialized(path: &Path, file: &mut TaskFile) -> Result<String, String> {
    file.version = FILE_VERSION;
    file.max_events_per_task = MAX_EVENTS_PER_TASK;
    file.events.sort_by_key(|e| e.created_at);
    enforce_event_limit(file);
    let text =
        serde_json::to_string_pretty(file).map_err(|e| format!("io: 序列化任务数据失败: {}", e))?;
    if path.as_os_str().is_empty() {
        // 无用户目录：纯内存降级，不落盘
        return Ok(text);
    }
    atomic_write(path, &text)?;
    Ok(text)
}

/// 写入前统一截断每个任务的事件到上限（R2 / AC5）
fn enforce_event_limit(file: &mut TaskFile) {
    let limit = file.max_events_per_task.max(1);
    // events 已按 created_at 升序；统计各任务超出量后从头丢
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for event in &file.events {
        *counts.entry(event.task_id.clone()).or_insert(0) += 1;
    }
    let mut drop_left: BTreeMap<String, usize> = BTreeMap::new();
    for (id, count) in counts {
        if count > limit {
            drop_left.insert(id, count - limit);
        }
    }
    if drop_left.is_empty() {
        return;
    }
    file.events.retain(|event| match drop_left.get_mut(&event.task_id) {
        Some(left) if *left > 0 => {
            *left -= 1;
            false
        }
        _ => true,
    });
}

/// 追加一条事件并裁剪该任务的事件数；返回新增事件
fn push_event(
    file: &mut TaskFile,
    task_id: &str,
    kind: TaskEventKind,
    actor: TaskActor,
    payload: Option<serde_json::Value>,
) -> WorkTaskEvent {
    let event = WorkTaskEvent {
        id: create_id("evt"),
        task_id: task_id.to_string(),
        kind,
        actor,
        payload,
        created_at: now_ms(),
    };
    file.events.push(event.clone());
    enforce_event_limit(file);
    event
}

/// 按 ID 找任务（可变）
fn find_task_mut<'a>(file: &'a mut TaskFile, id: &str) -> Result<&'a mut WorkTask, String> {
    file.tasks
        .iter_mut()
        .find(|t| t.id == id)
        .ok_or_else(|| "not-found".to_string())
}

/// 只读查找（单测断言用）
#[cfg(test)]
fn find_task<'a>(file: &'a TaskFile, id: &str) -> Result<&'a WorkTask, String> {
    file.tasks
        .iter()
        .find(|t| t.id == id)
        .ok_or_else(|| "not-found".to_string())
}

fn events_of(file: &TaskFile, task_id: &str) -> Vec<WorkTaskEvent> {
    let mut events: Vec<WorkTaskEvent> = file
        .events
        .iter()
        .filter(|e| e.task_id == task_id)
        .cloned()
        .collect();
    events.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
    events
}

fn illegal_transition(from: TaskStatus, to: TaskStatus) -> String {
    format!(
        "illegal-transition: {}->{}",
        from.as_wire(),
        to.as_wire()
    )
}

/// 时间戳副作用（prd R4）：queued → runSeq+1 且 startedAt 首次写入；
/// review/failed → settledAt；done/canceled → finishedAt。
///
/// 数据层对 `todo` 的生效范围（子任务 3 修正）：只有 **从 review / failed 打回** 才清
/// `settledAt`。`queued → todo`（启动链路失败回退）与 `canceled → todo`（重新打开）
/// 不是「打回」，`settledAt` 必须保持原值——若在这里无条件 `settled_at = None`，
/// 一个已结算过的任务在启动回退后时间线会出现「结算时间凭空消失」。
fn apply_entry_effects(task: &mut WorkTask, to: TaskStatus, now: i64, from: TaskStatus) {
    if to == TaskStatus::Queued {
        task.run_seq += 1;
        if task.started_at.is_none() {
            task.started_at = Some(now);
        }
    }
    if matches!(to, TaskStatus::Review | TaskStatus::Failed) {
        task.settled_at = Some(now);
    }
    if to == TaskStatus::Todo && matches!(from, TaskStatus::Review | TaskStatus::Failed) {
        task.settled_at = None;
    }
    if matches!(to, TaskStatus::Done | TaskStatus::Canceled) {
        task.finished_at = Some(now);
    }
}

/// 转换内核：CAS（expectedFrom）→ 表校验 → 副作用 → 事件
fn transition_task(
    file: &mut TaskFile,
    id: &str,
    to: TaskStatus,
    expected_from: Option<TaskStatus>,
) -> Result<WorkTask, String> {
    let now = now_ms();
    let task = find_task_mut(file, id)?;
    let from = task.status;
    if let Some(expected) = expected_from {
        if expected != from {
            return Err("stale".into());
        }
    }
    if !can_transition(from, to) {
        return Err(illegal_transition(from, to));
    }
    if from == to {
        return Ok(task.clone());
    }
    task.status = to;
    task.updated_at = now;
    apply_entry_effects(task, to, now, from);
    let updated = task.clone();
    push_event(
        file,
        id,
        TaskEventKind::StatusChanged,
        TaskActor::User,
        Some(serde_json::json!({ "from": from.as_wire(), "to": to.as_wire() })),
    );
    Ok(updated)
}

/// turn 回流内核（prd R5）：在**一个**写事务里完成「runSeq 校验 → 状态转移 →
/// lastError / failureReason → status_changed 事件（actor = engine）」。
/// 不用先 transition 再 update：那样中间会被另一个命令看到「已 review 但还没写归因」的
/// 半成品行，而 `task_update` 只允许 todo/failed 编辑，路径也走不通。
fn settle_turn_inner(
    file: &mut TaskFile,
    id: &str,
    run_seq: u64,
    outcome: TaskTurnOutcome,
    reason: Option<String>,
) -> Result<WorkTask, String> {
    let task = find_task_mut(file, id)?;
    // 迟到的 turn 回调：runSeq 已推进（任务被重试/再次启动）→ 静默丢弃，不改任何字段
    if task.run_seq != run_seq {
        return Ok(task.clone());
    }
    if outcome == TaskTurnOutcome::Cancelled {
        return Ok(task.clone());
    }
    // 只结算正在跑的任务；其它状态（用户已打回/取消）说明结论已过期，静默忽略
    if task.status != TaskStatus::Running {
        return Ok(task.clone());
    }
    let to = if outcome == TaskTurnOutcome::Ok {
        TaskStatus::Review
    } else {
        TaskStatus::Failed
    };
    let from = task.status;
    let now = now_ms();
    task.status = to;
    task.updated_at = now;
    match outcome {
        TaskTurnOutcome::Ok => {
            task.failure_reason = None;
            task.last_error = None;
        }
        TaskTurnOutcome::Empty => {
            task.failure_reason = Some(TaskFailureReason::EmptyTurn);
            task.last_error = reason.or_else(|| Some("Agent 未返回任何内容".to_string()));
        }
        TaskTurnOutcome::Error => {
            task.failure_reason = Some(TaskFailureReason::AgentError);
            task.last_error = reason;
        }
        TaskTurnOutcome::Cancelled => unreachable!("Cancelled 已在上方提前返回"),
    }
    apply_entry_effects(task, to, now, from);
    let updated = task.clone();
    push_event(
        file,
        id,
        TaskEventKind::StatusChanged,
        TaskActor::Engine,
        Some(serde_json::json!({ "from": from.as_wire(), "to": to.as_wire() })),
    );
    Ok(updated)
}

/// 解绑会话内核（「断开会话」）：一对一绑定，传当前 sessionId 做 CAS。
/// 生成中拒绝——用户必须先停止，否则会话进程会失去归属。
fn detach_session_inner(
    file: &mut TaskFile,
    id: &str,
    session_id: &str,
) -> Result<WorkTask, String> {
    let now = now_ms();
    let task = find_task_mut(file, id)?;
    if task.session_id.as_deref() != Some(session_id) {
        return Err("stale".into());
    }
    if matches!(
        task.status,
        TaskStatus::Queued | TaskStatus::Preparing | TaskStatus::Running
    ) {
        return Err("io: 任务正在执行，请先停止再断开会话".into());
    }
    task.session_id = None;
    task.updated_at = now;
    let updated = task.clone();
    push_event(
        file,
        id,
        TaskEventKind::UserAction,
        TaskActor::User,
        Some(serde_json::json!({ "action": "detach_session" })),
    );
    Ok(updated)
}

// ---------- Tauri commands ----------

/// 列出任务（可选按项目过滤）；返回全部（列表由前端派生，pinia 只存一份）
#[tauri::command]
pub fn task_list(project_id: Option<String>) -> Result<Vec<WorkTask>, String> {
    with_file_read(|data| {
        let mut tasks: Vec<WorkTask> = data
            .file
            .tasks
            .iter()
            .filter(|t| match project_id.as_deref() {
                Some(p) => t.project_id == p,
                None => true,
            })
            .cloned()
            .collect();
        // sortOrder 是项目内语义：跨项目混合时仍按更新时间倒序（同 codeg byFreshest）
        match project_id {
            Some(_) => tasks.sort_by_key(|t| t.sort_order),
            None => tasks.sort_by_key(|t| std::cmp::Reverse(t.updated_at)),
        }
        tasks
    })
}

/// 新建任务：追加到该项目列尾，status = todo
#[tauri::command]
pub fn task_create(draft: TaskDraft) -> Result<TaskDetails, String> {
    if draft.title.trim().is_empty() {
        return Err("io: 任务标题不能为空".into());
    }
    with_file_write(|file| {
        let now = now_ms();
        let sort_order = file
            .tasks
            .iter()
            .filter(|t| t.project_id == draft.project_id)
            .map(|t| t.sort_order)
            .max()
            .map_or(0, |m| m + 1);
        // 32 位 ID 极小概率撞号：与既有行去重再落库
        let mut id = create_id("task");
        while file.tasks.iter().any(|t| t.id == id) {
            id = create_id("task");
        }
        let task = WorkTask {
            id,
            project_id: draft.project_id.clone(),
            title: draft.title.trim().to_string(),
            prompt: draft.prompt.clone(),
            status: TaskStatus::Todo,
            sort_order,
            run_seq: 0,
            session_id: None,
            engine_id: draft.engine_id.clone(),
            verdict: None,
            result_summary: None,
            last_error: None,
            failure_reason: None,
            archived_at: None,
            created_at: now,
            updated_at: now,
            started_at: None,
            settled_at: None,
            finished_at: None,
        };
        file.tasks.push(task.clone());
        let event = push_event(file, &task.id, TaskEventKind::Created, TaskActor::User, None);
        Ok(TaskDetails {
            task,
            events: vec![event],
        })
    })
}

/// 编辑任务：仅 todo / failed 允许改 title / prompt / engineId
#[tauri::command]
pub fn task_update(id: String, patch: TaskPatch) -> Result<WorkTask, String> {
    with_file_write(|file| {
        let now = now_ms();
        let task = find_task_mut(file, &id)?;
        if !matches!(task.status, TaskStatus::Todo | TaskStatus::Failed) {
            return Err(format!(
                "io: 当前状态 {} 不允许编辑任务",
                task.status.as_wire()
            ));
        }
        if let Some(title) = patch.title {
            let trimmed = title.trim().to_string();
            if trimmed.is_empty() {
                return Err("io: 任务标题不能为空".into());
            }
            task.title = trimmed;
        }
        if let Some(prompt) = patch.prompt {
            task.prompt = prompt;
        }
        if let Some(engine) = patch.engine_id {
            task.engine_id = Some(engine);
        }
        task.updated_at = now;
        let updated = task.clone();
        push_event(
            file,
            &id,
            TaskEventKind::UserAction,
            TaskActor::User,
            Some(serde_json::json!({ "action": "updated" })),
        );
        Ok(updated)
    })
}

/// 硬删任务 + 其事件
#[tauri::command]
pub fn task_delete(id: String) -> Result<(), String> {
    with_file_write(|file| {
        let before = file.tasks.len();
        file.tasks.retain(|t| t.id != id);
        if file.tasks.len() == before {
            return Err("not-found".into());
        }
        file.events.retain(|e| e.task_id != id);
        Ok(())
    })
}

/// 状态转移（CAS）：expectedFrom 不匹配 → stale；表不许 → illegal-transition
#[tauri::command]
pub fn task_transition(
    id: String,
    to: TaskStatus,
    expected_from: Option<TaskStatus>,
) -> Result<WorkTask, String> {
    with_file_write(|file| transition_task(file, &id, to, expected_from))
}

/// 列内重排：按 index 写 sortOrder；别的项目的 id 静默忽略（作用域过滤）
#[tauri::command]
pub fn task_reorder(project_id: String, ordered_ids: Vec<String>) -> Result<Vec<WorkTask>, String> {
    with_file_write(|file| {
        let now = now_ms();
        for (index, id) in ordered_ids.iter().enumerate() {
            if let Some(task) = file
                .tasks
                .iter_mut()
                .find(|t| &t.id == id && t.project_id == project_id)
            {
                task.sort_order = index as i64;
                task.updated_at = now;
            }
        }
        let mut rows: Vec<WorkTask> = file
            .tasks
            .iter()
            .filter(|t| t.project_id == project_id)
            .cloned()
            .collect();
        rows.sort_by_key(|t| t.sort_order);
        Ok(rows)
    })
}

/// 绑定会话（启动链路用）：一对一，重新绑定时覆盖
#[tauri::command]
pub fn task_attach_session(
    id: String,
    session_id: String,
    engine_id: Option<String>,
) -> Result<WorkTask, String> {
    with_file_write(|file| {
        let now = now_ms();
        let task = find_task_mut(file, &id)?;
        task.session_id = Some(session_id.clone());
        if let Some(engine) = engine_id {
            task.engine_id = Some(engine);
        }
        task.updated_at = now;
        let updated = task.clone();
        push_event(
            file,
            &id,
            TaskEventKind::UserAction,
            TaskActor::User,
            Some(serde_json::json!({ "action": "attach_session", "sessionId": session_id })),
        );
        Ok(updated)
    })
}

/// 归档 / 取消归档：仅终态（done / canceled / failed）可归档
#[tauri::command]
pub fn task_archive(id: String, archived: bool) -> Result<WorkTask, String> {
    with_file_write(|file| {
        let now = now_ms();
        let task = find_task_mut(file, &id)?;
        if archived && !task.status.is_terminal() {
            return Err(format!(
                "io: 任务状态 {} 不可归档",
                task.status.as_wire()
            ));
        }
        task.archived_at = if archived { Some(now) } else { None };
        task.updated_at = now;
        let updated = task.clone();
        push_event(
            file,
            &id,
            TaskEventKind::UserAction,
            TaskActor::User,
            Some(serde_json::json!({ "action": if archived { "archive" } else { "unarchive" } })),
        );
        Ok(updated)
    })
}

/// 时间线（升序）
#[tauri::command]
pub fn task_events(id: String) -> Result<Vec<WorkTaskEvent>, String> {
    with_file_read(|data| {
        // 不存在视为空时间线（详情抽屉对已删任务容错）
        events_of(&data.file, &id)
    })
}

/// turn 回流（子任务 3 / prd R5）：把一次 agent turn 的结论写回任务。
/// `run_seq` 是**启动时**任务行的 runSeq（前端从 `transition → queued` 的返回里取）；
/// 与当前行不符即为迟到回调 → 原样返回当前行（静默忽略，不报错）。
/// `outcome = cancelled` 一律不改状态（用户主动停止任务保持 running，prd R4）。
#[tauri::command]
pub fn task_settle_turn(
    id: String,
    run_seq: u64,
    outcome: TaskTurnOutcome,
    reason: Option<String>,
) -> Result<WorkTask, String> {
    with_file_write(|file| settle_turn_inner(file, &id, run_seq, outcome, reason))
}

/// 解绑会话（详情「断开会话」）：任务与会话一对一（prd D4），解绑后下次启动走新建。
/// 生成中的任务先停止再解绑（否则会留下一个没有归属的进程）；`sessionId` 传当前值，
/// 与既有行不符 → stale（防并发）。
#[tauri::command]
pub fn task_detach_session(
    id: String,
    session_id: String,
) -> Result<WorkTask, String> {
    with_file_write(|file| detach_session_inner(file, &id, &session_id))
}

/// 复制任务（详情「再做一个」）：同项目、同提示词、同引擎，新行回到 todo。
/// 返回完整视图，调用方可直接打开新任务详情。
#[tauri::command]
pub fn task_duplicate(id: String) -> Result<TaskDetails, String> {
    let draft = with_file_read(|data| -> Result<TaskDraft, String> {
        // 复制 = 读源行 → 用同一 draft 形状重新创建（不含任何运行态字段）
        let source = data
            .file
            .tasks
            .iter()
            .find(|t| t.id == id)
            .ok_or_else(|| "not-found".to_string())?;
        Ok(TaskDraft {
            project_id: source.project_id.clone(),
            title: source.title.clone(),
            prompt: source.prompt.clone(),
            engine_id: source.engine_id.clone(),
        })
    })??;
    task_create(draft)
}

// ---------- 状态广播 ----------
//
// 变更后由命令层广播 `task://changed`（子任务 3 用它刷新会话联动后的列表）。
// 注意：命令层目前不直接持有 AppHandle，广播入口保留给将来接入；
// 前端 store 每次变更都 await 返回值更新本地行，不依赖事件。

/// `task://changed` 事件载荷
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskChanged<'a> {
    pub task_id: &'a str,
}

/// 广播任务变更（未注册监听时静默）
#[allow(dead_code)]
pub fn broadcast<R: tauri::Runtime>(app: &tauri::AppHandle<R>, task_id: &str) {
    if let Err(e) = app.emit("task://changed", TaskChanged { task_id }) {
        eprintln!("[task] 广播 task://changed 失败: {}", e);
    }
}

// ---------- 单元测试 ----------

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path(tag: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("lightr-task-test-{}-{}", tag, now_ms()))
            .join("tasks.json")
    }

    /// 走命令层（共享 static `TASK_STORE`）的测试必须串行：cargo test 默认多线程，
    /// 而 `TEST_PATH` + store 是进程级单例，并发用例会互相清空内存态。
    /// 用 `#[test]` 而非 `lock().unwrap()` 直接嵌套，避免某个用例 panic 后中毒级联失败。
    static STORE_LOCK: Mutex<()> = Mutex::new(());

    fn lock_store() -> std::sync::MutexGuard<'static, ()> {
        STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 把共享 store 指向临时路径并清空内存态（进入用例前）
    fn point_store_at(path: &Path) {
        *TEST_PATH.lock().unwrap() = Some(path.to_path_buf());
        *store().path.lock().unwrap() = None;
        *store().data.lock().unwrap() = None;
    }

    /// 退出用例：解除路径覆盖并清空内存态，避免影响下一个用例
    fn reset_store() {
        *TEST_PATH.lock().unwrap() = None;
        *store().path.lock().unwrap() = None;
        *store().data.lock().unwrap() = None;
    }

    /// 直接用内存 TaskFile 测试状态机（不碰磁盘）
    fn fresh() -> TaskFile {
        TaskFile {
            version: FILE_VERSION,
            tasks: Vec::new(),
            events: Vec::new(),
            max_events_per_task: MAX_EVENTS_PER_TASK,
            extra: BTreeMap::new(),
        }
    }

    fn make_task(file: &mut TaskFile, id: &str) -> WorkTask {
        let now = now_ms();
        let task = WorkTask {
            id: id.to_string(),
            project_id: "proj-a".into(),
            title: "t".into(),
            prompt: "p".into(),
            status: TaskStatus::Todo,
            sort_order: file.tasks.len() as i64,
            run_seq: 0,
            session_id: None,
            engine_id: None,
            verdict: None,
            result_summary: None,
            last_error: None,
            failure_reason: None,
            archived_at: None,
            created_at: now,
            updated_at: now,
            started_at: None,
            settled_at: None,
            finished_at: None,
        };
        file.tasks.push(task.clone());
        task
    }

    /// 把任务推进到 running（启动链路的前半段），返回 queued 时的 runSeq
    fn drive_to_running(file: &mut TaskFile, id: &str) -> u64 {
        let queued = transition_task(file, id, TaskStatus::Queued, Some(TaskStatus::Todo)).unwrap();
        transition_task(file, id, TaskStatus::Preparing, Some(TaskStatus::Queued)).unwrap();
        transition_task(file, id, TaskStatus::Running, Some(TaskStatus::Preparing)).unwrap();
        queued.run_seq
    }

    /// turn 回流：ok → review；empty → failed + empty_turn；error → failed + agent_error（R5）
    #[test]
    fn settle_turn_maps_outcomes() {
        // ok
        let mut file = fresh();
        make_task(&mut file, "task-1");
        let run = drive_to_running(&mut file, "task-1");
        let t = settle_turn_inner(&mut file, "task-1", run, TaskTurnOutcome::Ok, None).unwrap();
        assert_eq!(t.status, TaskStatus::Review);
        assert!(t.settled_at.is_some(), "review 应写 settledAt");
        assert_eq!(t.failure_reason, None);
        // 回流写入的 running→review 事件（前面几次转移都是 user，按 actor 取 engine 那条）
        let settle_event = events_of(&file, "task-1")
            .into_iter()
            .find(|e| e.actor == TaskActor::Engine)
            .expect("回流应写一条 engine 事件");
        assert_eq!(settle_event.kind, TaskEventKind::StatusChanged);
        assert_eq!(settle_event.payload.as_ref().unwrap()["to"], "review");
        assert_eq!(settle_event.payload.as_ref().unwrap()["from"], "running");
        // empty
        let mut file = fresh();
        make_task(&mut file, "task-1");
        let run = drive_to_running(&mut file, "task-1");
        let t = settle_turn_inner(&mut file, "task-1", run, TaskTurnOutcome::Empty, None).unwrap();
        assert_eq!(t.status, TaskStatus::Failed);
        assert_eq!(t.failure_reason, Some(TaskFailureReason::EmptyTurn));
        assert!(t.last_error.is_some(), "空回复应留下可读原因");

        // error：保留调用方给的原文
        let mut file = fresh();
        make_task(&mut file, "task-1");
        let run = drive_to_running(&mut file, "task-1");
        let t = settle_turn_inner(
            &mut file,
            "task-1",
            run,
            TaskTurnOutcome::Error,
            Some("生成失败：boom".into()),
        )
        .unwrap();
        assert_eq!(t.status, TaskStatus::Failed);
        assert_eq!(t.failure_reason, Some(TaskFailureReason::AgentError));
        assert_eq!(t.last_error.as_deref(), Some("生成失败：boom"));
    }

    /// runSeq 防串：迟到回调（runSeq 已推进）不改任何字段（R5）
    #[test]
    fn settle_turn_drops_stale_run_seq() {
        let mut file = fresh();
        make_task(&mut file, "task-1");
        let old_run = drive_to_running(&mut file, "task-1");
        // 用户停止 → 取消 → 重新打开 → 再次启动：runSeq 推进
        transition_task(&mut file, "task-1", TaskStatus::Canceled, Some(TaskStatus::Running)).unwrap();
        transition_task(&mut file, "task-1", TaskStatus::Todo, Some(TaskStatus::Canceled)).unwrap();
        let new_run = drive_to_running(&mut file, "task-1");
        assert_eq!(new_run, old_run + 1);

        let event_count = events_of(&file, "task-1").len();
        let t = settle_turn_inner(
            &mut file,
            "task-1",
            old_run,
            TaskTurnOutcome::Error,
            Some("旧 turn 迟到的失败".into()),
        )
        .unwrap();
        assert_eq!(t.status, TaskStatus::Running, "旧 turn 不得改动新 run 的状态");
        assert_eq!(t.failure_reason, None);
        assert_eq!(events_of(&file, "task-1").len(), event_count, "迟到回调不写事件");
    }

    /// cancelled 不改状态；非 running 状态的回流静默忽略（R4）
    #[test]
    fn settle_turn_ignores_cancelled_and_non_running() {
        let mut file = fresh();
        make_task(&mut file, "task-1");
        let run = drive_to_running(&mut file, "task-1");
        let t = settle_turn_inner(&mut file, "task-1", run, TaskTurnOutcome::Cancelled, None).unwrap();
        assert_eq!(t.status, TaskStatus::Running, "用户主动停止保持 running");
        assert_eq!(t.settled_at, None);

        // 用户已打回（review→todo）后 turn 才迟到：属于过去时，不改状态
        settle_turn_inner(&mut file, "task-1", run, TaskTurnOutcome::Ok, None).unwrap();
        transition_task(&mut file, "task-1", TaskStatus::Todo, Some(TaskStatus::Review)).unwrap();
        let t = settle_turn_inner(&mut file, "task-1", run, TaskTurnOutcome::Error, None)
            .unwrap();
        assert_eq!(t.status, TaskStatus::Todo, "非 running 忽略回流的失败结论");

        let err = settle_turn_inner(&mut file, "nope", run, TaskTurnOutcome::Ok, None).unwrap_err();
        assert_eq!(err, "not-found");
    }

    /// 解绑会话：生成中拒绝（先停止）；sessionId 不符 stale；成功后清空（R2）
    #[test]
    fn detach_session_rules() {
        let mut file = fresh();
        make_task(&mut file, "task-1");
        find_task_mut(&mut file, "task-1").unwrap().session_id = Some("ses-1".into());

        // sessionId 不符 → stale（并发保护）
        assert_eq!(
            detach_session_inner(&mut file, "task-1", "ses-2").unwrap_err(),
            "stale"
        );

        // todo 允许
        let t = detach_session_inner(&mut file, "task-1", "ses-1").unwrap();
        assert_eq!(t.session_id, None);

        // running 拒绝
        find_task_mut(&mut file, "task-1").unwrap().session_id = Some("ses-3".into());
        drive_to_running(&mut file, "task-1");
        let err = detach_session_inner(&mut file, "task-1", "ses-3").unwrap_err();
        assert!(err.starts_with("io: "), "实际 {}", err);
    }

    /// 转换表穷尽：每个 from 的每条出边为真，未列出的目标为假（AC1）
    #[test]
    fn transition_table_is_exhaustive() {
        for &from in TaskStatus::ALL {
            let allowed = TRANSITIONS
                .iter()
                .find(|(s, _)| *s == from)
                .map(|(_, targets)| *targets)
                .unwrap_or(&[]);
            for &to in TaskStatus::ALL {
                let expected = allowed.contains(&to) || from == to;
                assert_eq!(
                    can_transition(from, to),
                    expected,
                    "{} -> {} 期望 {} 实际 {}",
                    from.as_wire(),
                    to.as_wire(),
                    expected,
                    can_transition(from, to)
                );
            }
        }
        // 表本身覆盖全部状态（无遗漏 = 新增状态必须归表，否则此断言失败）
        assert_eq!(TRANSITIONS.len(), TaskStatus::ALL.len());
    }

    /// 打回与非法跨列：review→todo 合法，done→todo / todo→review 非法（AC2）
    #[test]
    fn illegal_cross_column_transitions() {
        assert!(can_transition(TaskStatus::Review, TaskStatus::Todo));
        assert!(can_transition(TaskStatus::Review, TaskStatus::Done));
        assert!(can_transition(TaskStatus::Todo, TaskStatus::Queued));
        assert!(!can_transition(TaskStatus::Done, TaskStatus::Todo));
        assert!(!can_transition(TaskStatus::Todo, TaskStatus::Review));
        assert!(!can_transition(TaskStatus::Todo, TaskStatus::Running));
        assert!(!can_transition(TaskStatus::Done, TaskStatus::Canceled));
        assert!(!can_transition(TaskStatus::Merging, TaskStatus::Done));
    }

    /// 时间戳副作用：queued 置 runSeq/startedAt；review 置 settledAt；done 置 finishedAt（R4）
    #[test]
    fn transition_sets_timestamps_and_run_seq() {
        let mut file = fresh();
        make_task(&mut file, "task-1");

        let t = transition_task(&mut file, "task-1", TaskStatus::Queued, Some(TaskStatus::Todo))
            .expect("todo→queued 应成功");
        assert_eq!(t.status, TaskStatus::Queued);
        assert_eq!(t.run_seq, 1, "进入 queued 时 runSeq +1");
        assert!(t.started_at.is_some(), "首次 startedAt 应写入");
        let first_started = t.started_at;

        let t = transition_task(&mut file, "task-1", TaskStatus::Preparing, Some(TaskStatus::Queued))
            .unwrap();
        assert_eq!(t.run_seq, 1, "非 queued 不增 runSeq");
        let _ = t;
        transition_task(&mut file, "task-1", TaskStatus::Running, Some(TaskStatus::Preparing))
            .unwrap();
        let t = transition_task(&mut file, "task-1", TaskStatus::Review, Some(TaskStatus::Running))
            .unwrap();
        assert!(t.settled_at.is_some(), "进入 review 应写 settledAt");
        let settled = t.settled_at;

        // 打回 → 再次入队：runSeq 累加，startedAt 保留首次，settledAt 被清（重新开始计时）
        let t = transition_task(&mut file, "task-1", TaskStatus::Todo, Some(TaskStatus::Review))
            .unwrap();
        assert_eq!(t.settled_at, None, "打回待办应清 settledAt");
        assert!(settled.is_some());
        let t = transition_task(&mut file, "task-1", TaskStatus::Queued, Some(TaskStatus::Todo))
            .unwrap();
        assert_eq!(t.run_seq, 2);
        assert_eq!(t.started_at, first_started, "startedAt 只写首次");
        assert_eq!(t.settled_at, None, "queued→todo 回退不是打回，但此时本就没有 settledAt");
        transition_task(&mut file, "task-1", TaskStatus::Preparing, Some(TaskStatus::Queued))
            .unwrap();
        let t = transition_task(&mut file, "task-1", TaskStatus::Failed, Some(TaskStatus::Preparing))
            .unwrap();
        assert!(t.settled_at.is_some(), "进入 failed 应写 settledAt");
        let t = transition_task(&mut file, "task-1", TaskStatus::Canceled, Some(TaskStatus::Failed))
            .unwrap();
        assert!(t.finished_at.is_some(), "进入 canceled 应写 finishedAt");
    }

    /// illegal-transition 与 stale 错误串（AC2/AC3）
    #[test]
    fn transition_error_strings() {
        let mut file = fresh();
        make_task(&mut file, "task-1");

        let err = transition_task(&mut file, "task-1", TaskStatus::Review, Some(TaskStatus::Todo))
            .unwrap_err();
        assert_eq!(err, "illegal-transition: todo->review");

        // stale：期望与实际不符，且行未被修改
        let err = transition_task(
            &mut file,
            "task-1",
            TaskStatus::Queued,
            Some(TaskStatus::Running),
        )
        .unwrap_err();
        assert_eq!(err, "stale");
        assert_eq!(find_task(&file, "task-1").unwrap().status, TaskStatus::Todo);
        assert!(file.events.is_empty(), "失败转移不写事件");

        let err = transition_task(&mut file, "nope", TaskStatus::Queued, None).unwrap_err();
        assert_eq!(err, "not-found");

        // done → todo 非法
        let task = find_task_mut(&mut file, "task-1").unwrap();
        task.status = TaskStatus::Done;
        let err = transition_task(&mut file, "task-1", TaskStatus::Todo, None).unwrap_err();
        assert_eq!(err, "illegal-transition: done->todo");
    }

    /// 每次成功转移写一条 status_changed，payload = {from, to}
    #[test]
    fn transition_writes_status_changed_event() {
        let mut file = fresh();
        make_task(&mut file, "task-1");
        transition_task(&mut file, "task-1", TaskStatus::Queued, Some(TaskStatus::Todo)).unwrap();
        let events = events_of(&file, "task-1");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, TaskEventKind::StatusChanged);
        let payload = events[0].payload.as_ref().unwrap();
        assert_eq!(payload["from"], "todo");
        assert_eq!(payload["to"], "queued");
    }

    /// 事件截断：写 250 次转移后该任务只剩 200 条（AC5）
    #[test]
    fn events_truncated_per_task() {
        let mut file = fresh();
        make_task(&mut file, "task-1");
        make_task(&mut file, "task-2");
        for i in 0..250 {
            let to = if i % 2 == 0 {
                TaskStatus::Queued
            } else {
                TaskStatus::Todo
            };
            transition_task(&mut file, "task-1", to, None).unwrap();
            // 触发裁剪（写入时执行；这里用同一函数模拟）
            push_event(
                &mut file,
                "task-1",
                TaskEventKind::AgentProgress,
                TaskActor::Agent,
                None,
            );
        }
        let task1_events = file.events.iter().filter(|e| e.task_id == "task-1").count();
        assert!(
            task1_events <= MAX_EVENTS_PER_TASK,
            "单任务事件未截断: {}",
            task1_events
        );
        // 另一个任务的事件不受影响
        assert_eq!(
            file.events.iter().filter(|e| e.task_id == "task-2").count(),
            0
        );
    }

    /// 落盘：损坏文件不覆盖，写入前备份为 .bak（R2）
    #[test]
    fn corrupt_file_is_backed_up_not_silently_overwritten() {
        let path = tmp_path("corrupt");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{ this is not json").unwrap();
        let bak = path.with_file_name("tasks.json.bak");

        // 读：损坏 → 空数据 + corrupt，磁盘原文**不被**改动
        let mut data = load_from_disk(&path);
        assert!(data.corrupt, "损坏文件应标记 corrupt");
        assert!(data.file.tasks.is_empty(), "损坏文件读为空数据");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "{ this is not json",
            "仅读取不得改写磁盘"
        );
        assert!(!bak.exists(), "读取时不应产生备份");

        // 首次写回：备份必须在主文件被覆盖**之前**完成 → bak 是原文
        let text = save_serialized(&path, &mut data.file).unwrap();
        backup_corrupt(&path, &data.raw);
        data.committed(text);
        assert_eq!(
            fs::read_to_string(&bak).unwrap(),
            "{ this is not json",
            "bak 必须是损坏原文，而非写回后的新内容"
        );
        let reread = load_from_disk(&path);
        assert!(!reread.corrupt, "写回后主文件应可解析");
        assert!(reread.file.tasks.is_empty());

        // 快照已提交：再次写回不应重复备份
        assert!(!data.corrupt);
        fs::remove_file(&bak).unwrap();
        let text = save_serialized(&path, &mut data.file).unwrap();
        assert!(!data.corrupt);
        data.committed(text);
        assert!(!bak.exists(), "合法数据写回不应再次备份");

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    /// 写事务链路：损坏文件经 `with_file_write` 写回后，bak 保存的是原文（顺序回归）
    #[test]
    fn corrupt_backup_happens_before_overwrite() {
        let _serial = lock_store();
        let root = std::env::temp_dir().join(format!("lightr-task-bakorder-{}", now_ms()));
        let path = root.join("tasks.json");
        fs::create_dir_all(&root).unwrap();
        let broken = "{\"tasks\": [ 坏的";
        fs::write(&path, broken).unwrap();
        point_store_at(&path);

        let created = task_create(TaskDraft {
            project_id: "proj-a".into(),
            title: "坏了也要能建".into(),
            prompt: String::new(),
            engine_id: None,
        })
        .unwrap();
        assert_eq!(created.task.status, TaskStatus::Todo);

        let bak = path.with_file_name("tasks.json.bak");
        assert_eq!(
            fs::read_to_string(&bak).unwrap(),
            broken,
            "下次写入前应把损坏原文备份为 tasks.json.bak"
        );
        let reread = load_from_disk(&path);
        assert!(!reread.corrupt);
        assert_eq!(reread.file.tasks.len(), 1);

        reset_store();
        let _ = fs::remove_dir_all(root);
    }

    /// 未知顶层字段原样带回（design §2.2 前向兼容），已知字段按值写回
    #[test]
    fn unknown_top_level_fields_survive_roundtrip() {
        let path = tmp_path("extra");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let raw = r#"{"version":1,"tasks":[],"events":[],"futureFlag":{"a":1}}"#;
        fs::write(&path, raw).unwrap();

        let mut data = load_from_disk(&path);
        assert!(!data.corrupt);
        let text = save_serialized(&path, &mut data.file).unwrap();
        // 用数值/结构断言，不绑定缩进：flatten 是否让 serde_json 走 serialize_map
        // 属于实现细节，测试只保证「内容带回来」这个契约。
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            value["futureFlag"]["a"], 1,
            "未知顶层字段应原样带回: {}",
            text
        );
        assert_eq!(value["version"], 1, "版本号应写回 1");
        assert_eq!(
            value["max_events_per_task"], MAX_EVENTS_PER_TASK,
            "事件上限应显式写回: {}",
            text
        );

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    /// 落盘 roundtrip：写读一致 + 无临时文件残留（原子写）
    #[test]
    fn save_and_load_roundtrip() {
        let path = tmp_path("roundtrip");
        let mut file = fresh();
        make_task(&mut file, "task-1");
        transition_task(&mut file, "task-1", TaskStatus::Queued, Some(TaskStatus::Todo)).unwrap();
        save_serialized(&path, &mut file).unwrap();

        let loaded = load_from_disk(&path);
        assert_eq!(loaded.file.tasks.len(), 1);
        assert_eq!(loaded.file.tasks[0].status, TaskStatus::Queued);
        assert_eq!(loaded.file.tasks[0].run_seq, 1);
        assert_eq!(loaded.file.events.len(), 1);
        let leftovers: Vec<String> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains("lightr-tmp"))
            .collect();
        assert!(leftovers.is_empty(), "残留临时文件: {:?}", leftovers);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    /// 缺失文件 → 空数据且非损坏（R2：不存在不是错误）
    #[test]
    fn missing_file_loads_as_empty_without_error() {
        let path = tmp_path("missing");
        let data = load_from_disk(&path);
        assert!(!data.corrupt);
        assert!(data.file.tasks.is_empty());
        assert_eq!(data.file.version, 0, "空数据版本由落盘时补齐");
    }

    /// 重排作用域：别的项目的 id 静默忽略；本项目的按 index 写 sortOrder（AC4）
    #[test]
    fn reorder_is_scoped_to_project() {
        let mut file = fresh();
        let a1 = make_task(&mut file, "task-a1");
        let a2 = make_task(&mut file, "task-a2");
        let b1 = make_task(&mut file, "task-b1");
        // b1 归属另一个项目
        find_task_mut(&mut file, "task-b1").unwrap().project_id = "proj-b".into();

        let ordered = [a2.id.clone(), b1.id.clone(), a1.id.clone()];
        for (index, id) in ordered.iter().enumerate() {
            if let Some(task) = file
                .tasks
                .iter_mut()
                .find(|t| &t.id == id && t.project_id == "proj-a")
            {
                task.sort_order = index as i64;
            }
        }
        assert_eq!(find_task(&file, "task-a2").unwrap().sort_order, 0);
        assert_eq!(find_task(&file, "task-a1").unwrap().sort_order, 2);
        // 别的项目未被改动
        assert_eq!(find_task(&file, "task-b1").unwrap().sort_order, b1.sort_order);
    }

    /// 命令链路（含落盘 + 复用内存态）：创建 → 转移 → 列表 → 重排 → 删除。
    /// 通过 TEST_PATH 指向临时目录，不碰真实 tasks.json。
    #[test]
    fn commands_roundtrip_through_store() {
        let _serial = lock_store();
        let root = std::env::temp_dir().join(format!("lightr-task-cmd-{}", now_ms()));
        let path = root.join("tasks.json");
        point_store_at(&path);

        let created = task_create(TaskDraft {
            project_id: "proj-a".into(),
            title: "命令链路".into(),
            prompt: "do it".into(),
            engine_id: Some("open_code".into()),
        })
        .unwrap();
        assert_eq!(created.task.status, TaskStatus::Todo);
        assert_eq!(created.events.len(), 1, "创建应返回 created 事件");

        let id = created.task.id.clone();
        let queued = task_transition(id.clone(), TaskStatus::Queued, Some(TaskStatus::Todo)).unwrap();
        assert_eq!(queued.run_seq, 1);

        // 非法转移
        let err = task_transition(id.clone(), TaskStatus::Done, None).unwrap_err();
        assert!(err.starts_with("illegal-transition: "), "实际 {}", err);

        // stale
        let err = task_transition(id.clone(), TaskStatus::Preparing, Some(TaskStatus::Todo)).unwrap_err();
        assert_eq!(err, "stale");

        // 列表（项目内按 sortOrder）
        let listed = task_list(Some("proj-a".into())).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].session_id, None);

        // 绑定会话 + 归档拒绝（非终态）
        let attached = task_attach_session(id.clone(), "ses-1".into(), None).unwrap();
        assert_eq!(attached.session_id.as_deref(), Some("ses-1"));
        let err = task_archive(id.clone(), true).unwrap_err();
        assert!(err.starts_with("io: "), "实际 {}", err);

        // 时间线含创建 + 状态变更 + 绑定会话
        let timeline = task_events(id.clone()).unwrap();
        assert_eq!(timeline.len(), 3, "时间线: {:?}", timeline.len());

        // 重排 + 删除
        assert_eq!(task_reorder("proj-a".into(), vec![id.clone()]).unwrap().len(), 1);
        task_delete(id.clone()).unwrap();
        assert!(task_list(None).unwrap().is_empty());
        assert!(task_events(id).unwrap().is_empty(), "事件随任务删除");

        // 文件已落盘为合法 JSON
        let raw = fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["version"], 1);

        *TEST_PATH.lock().unwrap() = None;
        // 重置内存态，避免影响其他用例（同一进程共享 static）
        *store().path.lock().unwrap() = None;
        *store().data.lock().unwrap() = None;
        let _ = fs::remove_dir_all(root);
    }

    /// 重排命令的作用域：别的项目的 id 只是被忽略，不报错也不改（AC4，命令级）
    #[test]
    fn reorder_command_ignores_foreign_ids_without_error() {
        let _serial = lock_store();
        let root = std::env::temp_dir().join(format!("lightr-task-reorder-{}", now_ms()));
        let path = root.join("tasks.json");
        point_store_at(&path);

        let a = task_create(TaskDraft {
            project_id: "proj-a".into(),
            title: "A".into(),
            prompt: String::new(),
            engine_id: None,
        })
        .unwrap()
        .task;
        let b = task_create(TaskDraft {
            project_id: "proj-b".into(),
            title: "B".into(),
            prompt: String::new(),
            engine_id: None,
        })
        .unwrap()
        .task;

        // proj-a 的重排里混入 proj-b 的 id：静默忽略，不返回 Err
        let rows = task_reorder("proj-a".into(), vec![b.id.clone(), a.id.clone()]).unwrap();
        assert_eq!(rows.len(), 1, "只返回本项目行");
        assert_eq!(rows[0].id, a.id);

        let b_rows = task_list(Some("proj-b".into())).unwrap();
        assert_eq!(b_rows.len(), 1);
        assert_eq!(b_rows[0].id, b.id);
        assert_eq!(
            b_rows[0].sort_order, b.sort_order,
            "别的项目的 sortOrder 不得被改动"
        );

        reset_store();
        let _ = fs::remove_dir_all(root);
    }

    /// 序列化形状：camelCase 字段名（与前端 types/task.ts 对齐，PAC2）
    #[test]
    fn task_serializes_camel_case() {
        let mut file = fresh();
        make_task(&mut file, "task-1");
        let json = serde_json::to_string(&file.tasks[0]).unwrap();
        for key in [
            "\"projectId\"",
            "\"sortOrder\"",
            "\"runSeq\"",
            "\"sessionId\"",
            "\"engineId\"",
            "\"archivedAt\"",
            "\"createdAt\"",
            "\"settledAt\"",
            "\"finishedAt\"",
        ] {
            assert!(json.contains(key), "缺少 camelCase 字段 {}", key);
        }
        // 状态是 snake_case wire 名
        assert!(json.contains("\"status\":\"todo\""));
    }
}
