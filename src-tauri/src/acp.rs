// ACP (Agent Client Protocol) 进程管理：长驻单 `opencode acp` 子进程 + 多会话复用（design D1）。
// 传输：newline-delimited JSON-RPC 2.0 over stdio（research/acp-protocol.md §1，非 LSP 帧）。
// D2：不声明 client fs/terminal capability；对此类反向请求一律以 -32601 拒绝，
//     Agent 文件操作由 Lightr-vibe 自有文件面板承担（安全边界）。
// 错误串约定：`acp:<code> <message>` / `acp-dead: ...` / `timeout:<秒>s <method>`，
// 前端按前缀分支（tauri-bridge spec §2）。

use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// Windows 下子进程不弹控制台窗口
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// stderr 环形缓冲保留行数（进程死亡诊断用）
const STDERR_KEEP_LINES: usize = 50;

/// 请求超时矩阵（PRD R1）：initialize 10s / session/new、load 30s（首调实测
/// 7.6-8.9s，probe §2）/ prompt 300s / 其余 30s
fn timeout_for(method: &str) -> Duration {
    match method {
        "initialize" => Duration::from_secs(10),
        "session/new" | "session/load" => Duration::from_secs(30),
        "session/prompt" => Duration::from_secs(300),
        _ => Duration::from_secs(30),
    }
}

/// 入站消息三分路由（research/acp-protocol.md §2）：
/// 有 id 无 method = response；有 method 无 id = notification；两者都有 = agent→client request。
/// request 的 id 保留原始 Value（JSON-RPC 允许字符串 id；响应必须原样回显）
#[derive(Debug, PartialEq)]
enum Route {
    Response(u64),
    Notification(String),
    Request(Value, String),
    Unknown,
}

fn classify(msg: &Value) -> Route {
    let method = msg.get("method").and_then(Value::as_str);
    let id = msg.get("id");
    match (method, id) {
        // 我方只发整数 id，response 关联按 u64；无法解析（含 id:null 的错误响应）→ Unknown
        (None, Some(i)) => i.as_u64().map(Route::Response).unwrap_or(Route::Unknown),
        (Some(m), None) => Route::Notification(m.to_string()),
        (Some(m), Some(i)) if !i.is_null() => Route::Request(i.clone(), m.to_string()),
        _ => Route::Unknown,
    }
}

/// JSON-RPC error → 前端错误串（保留 code 供分支：-32000 凭据 / -32602 参数等）
fn rpc_error_string(err: &Value) -> String {
    let code = err.get("code").and_then(Value::as_i64).unwrap_or(-32603);
    let message = err
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("unknown error");
    format!("acp:{} {}", code, message)
}

/// 从 Windows .cmd shim 文本提取真身 exe 路径（version-fox 形态：
/// `"%dp0%\node_modules\opencode-ai\bin\opencode.exe" %*`，probe §0）。
/// 支持 `%dp0%` 与 `%~dp0` 两种占位符；替换后统一为单个路径分隔符。
fn extract_exe_from_shim(shim_text: &str, shim_dir: &str) -> Option<String> {
    let dir = shim_dir.trim_end_matches(['\\', '/']);
    let with_sep = format!("{}\\", dir);
    let mut in_quote = false;
    let mut token = String::new();
    for ch in shim_text.chars() {
        if ch == '"' {
            if in_quote {
                if token.to_ascii_lowercase().ends_with(".exe") {
                    let resolved = token
                        .replace("%~dp0\\", &with_sep)
                        .replace("%~dp0", &with_sep)
                        .replace("%dp0%\\", &with_sep)
                        .replace("%dp0%", &with_sep);
                    return Some(resolved);
                }
                token.clear();
            }
            in_quote = !in_quote;
        } else if in_quote {
            token.push(ch);
        }
    }
    None
}

enum SpawnSpec {
    /// 直接 spawn 真身 exe（probe §0 路线 1：长驻双向 stdio 进程最干净）
    Exe(PathBuf),
    /// 末选回退：`cmd /C opencode acp`（tauri-bridge Gotcha：.cmd 无法 CreateProcess）
    CmdFallback,
}

/// which 语义解析 opencode（与 agent.rs::resolve_bin 同逻辑；why 重复实现：
/// 不触碰设置页子任务的 agent.rs 文件面）
fn resolve_bin() -> Option<String> {
    let path_var = std::env::var("PATH").ok()?;
    let candidates: &[&str] = if cfg!(windows) {
        &["opencode.exe", "opencode.cmd", "opencode.bat", "opencode"]
    } else {
        &["opencode"]
    };
    for dir in std::env::split_paths(&path_var) {
        for name in candidates {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
    }
    None
}

/// spawn 解析链（PRD R1）：which opencode → .cmd shim 文本提取真身 exe → cmd /C 末选
fn resolve_spawn() -> SpawnSpec {
    if let Some(bin) = resolve_bin() {
        let path = Path::new(&bin);
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase());
        match ext.as_deref() {
            Some("cmd") | Some("bat") => {
                if let Ok(text) = std::fs::read_to_string(path) {
                    let dir = path
                        .parent()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    if let Some(exe) = extract_exe_from_shim(&text, &dir) {
                        if Path::new(&exe).is_file() {
                            return SpawnSpec::Exe(PathBuf::from(exe));
                        }
                    }
                }
            }
            _ => {
                if path.is_file() {
                    return SpawnSpec::Exe(path.to_path_buf());
                }
            }
        }
    }
    SpawnSpec::CmdFallback
}

// ---------- 连接对象 ----------

struct AcpConn {
    child: Mutex<Child>,
    /// Option：shutdown/清理时 take 掉即关闭 stdin（probe §4：stdin 关闭 → rc=0 干净退出）
    stdin: Mutex<Option<BufWriter<ChildStdin>>>,
    next_id: AtomicU64,
    /// JSON-RPC id → 等待响应的 oneshot（std mpsc）
    pending: Mutex<HashMap<u64, Sender<Value>>>,
    /// requestId → (JSON-RPC id 原样回显值, sessionId)：request_permission 等待前端答复
    pending_perm: Mutex<HashMap<String, (Value, String)>>,
    perm_next: AtomicU64,
    agent_info: Mutex<Value>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    dead: AtomicBool,
}

fn conn_slot() -> &'static Mutex<Option<Arc<AcpConn>>> {
    static CONN: OnceLock<Mutex<Option<Arc<AcpConn>>>> = OnceLock::new();
    CONN.get_or_init(|| Mutex::new(None))
}

fn dead_reason_slot() -> &'static Mutex<Option<String>> {
    static REASON: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    REASON.get_or_init(|| Mutex::new(None))
}

fn current_conn() -> Option<Arc<AcpConn>> {
    conn_slot().lock().unwrap().clone()
}

fn require_conn() -> Result<Arc<AcpConn>, String> {
    current_conn()
        .filter(|c| !c.dead.load(Ordering::SeqCst))
        .ok_or_else(|| "acp-dead: ACP 进程未启动，请先调用 acp_start".to_string())
}

fn send_line(conn: &AcpConn, value: &Value) -> Result<(), String> {
    let mut guard = conn.stdin.lock().unwrap();
    let writer = guard.as_mut().ok_or("acp-dead: 进程 stdin 已关闭")?;
    // 帧格式：一行一条完整 JSON + 裸 \n（协议 §1；Rust std 无 CRLF 翻译，writeln! 即 \n）
    writeln!(writer, "{}", value).map_err(|e| format!("acp-dead: 写入 stdin 失败: {}", e))?;
    writer
        .flush()
        .map_err(|e| format!("acp-dead: flush stdin 失败: {}", e))
}

fn register_pending(map: &mut HashMap<u64, Sender<Value>>, id: u64) -> Receiver<Value> {
    let (tx, rx) = mpsc::channel();
    map.insert(id, tx);
    rx
}

fn take_pending(map: &mut HashMap<u64, Sender<Value>>, id: u64) -> Option<Sender<Value>> {
    map.remove(&id)
}

/// 发请求并阻塞等待响应（调用方必须处于 spawn_blocking）；超时清理 pending 并返回 Err
fn rpc_call(conn: &AcpConn, method: &str, params: Value) -> Result<Value, String> {
    let id = conn.next_id.fetch_add(1, Ordering::SeqCst);
    let rx = {
        let mut pending = conn.pending.lock().unwrap();
        register_pending(&mut pending, id)
    };
    let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
    if let Err(e) = send_line(conn, &msg) {
        take_pending(&mut conn.pending.lock().unwrap(), id);
        return Err(e);
    }
    let timeout = timeout_for(method);
    match rx.recv_timeout(timeout) {
        Ok(resp) => {
            if let Some(err) = resp.get("error") {
                return Err(rpc_error_string(err));
            }
            Ok(resp.get("result").cloned().unwrap_or(Value::Null))
        }
        Err(RecvTimeoutError::Timeout) => {
            // 超时必须清理 pending；迟到响应到达时按 id 查不到即被丢弃
            take_pending(&mut conn.pending.lock().unwrap(), id);
            Err(format!("timeout:{}s {}", timeout.as_secs(), method))
        }
        Err(RecvTimeoutError::Disconnected) => {
            Err("acp-dead: 进程已退出（等待响应期间）".to_string())
        }
    }
}

// ---------- 读线程：三分路由 ----------

fn handle_incoming(app: &AppHandle, conn: &AcpConn, msg: &Value) {
    match classify(msg) {
        Route::Response(id) => {
            if let Some(tx) = take_pending(&mut conn.pending.lock().unwrap(), id) {
                let _ = tx.send(msg.clone());
            }
            // 无 pending（超时后迟到）的响应直接丢弃
        }
        Route::Notification(method) => {
            if method == "session/update" {
                let params = msg.get("params");
                let session_id = params
                    .and_then(|p| p.get("sessionId"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let update = params
                    .and_then(|p| p.get("update"))
                    .cloned()
                    .unwrap_or(Value::Null);
                let _ = app.emit(
                    "acp://update",
                    json!({ "sessionId": session_id, "update": update }),
                );
            } else if method == "$/cancel_request" {
                // 通用请求取消（协议 §2/§7）：被引用请求 MUST 收到响应。
                // opencode 实测不发此通知，按规范兜底：取消挂起的 permission 请求。
                if let Some(rid) = msg.get("params").and_then(|p| p.get("requestId")) {
                    cancel_by_rpc_id(conn, rid);
                }
            }
            // 其余 notification 本期忽略
        }
        Route::Request(id, method) => handle_client_request(conn, id, &method, msg, app),
        Route::Unknown => {}
    }
}

fn handle_client_request(
    conn: &AcpConn,
    id: Value,
    method: &str,
    msg: &Value,
    app: &AppHandle,
) {
    if method == "session/request_permission" {
        let request_id = conn.perm_next.fetch_add(1, Ordering::SeqCst).to_string();
        let params = msg.get("params");
        let session_id = params
            .and_then(|p| p.get("sessionId"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        conn.pending_perm
            .lock()
            .unwrap()
            .insert(request_id.clone(), (id, session_id.clone()));
        let _ = app.emit(
            "acp://permission",
            json!({
                "requestId": request_id,
                "sessionId": session_id,
                "toolCall": params.and_then(|p| p.get("toolCall")).cloned().unwrap_or(Value::Null),
                "options": params.and_then(|p| p.get("options")).cloned().unwrap_or(Value::Null),
            }),
        );
        return;
    }
    // D2：fs/terminal/elicitation 一律拒绝（initialize 未声明 capability，双保险；
    // Agent 的文件操作由自有文件面板观察，不走 Agent 借道写盘）
    if method.starts_with("fs/")
        || method.starts_with("terminal/")
        || method.starts_with("elicitation/")
    {
        let _ = send_line(
            conn,
            &json!({
                "jsonrpc": "2.0", "id": id,
                "error": { "code": -32601, "message": format!("unsupported method {}: 由 Lightr-vibe 文件面板承担", method) }
            }),
        );
        return;
    }
    let _ = send_line(
        conn,
        &json!({
            "jsonrpc": "2.0", "id": id,
            "error": { "code": -32601, "message": format!("method not found: {}", method) }
        }),
    );
}

/// $/cancel_request 兜底：按被取消请求的 JSON-RPC id 找 pending_perm 中对应条目，
/// 回 cancelled outcome 并从 map 移除（协议 §5：收到方 MUST 对原请求回响应；
/// permission 请求的规范取消形状是 outcome cancelled）
fn cancel_by_rpc_id(conn: &AcpConn, rpc_id: &Value) {
    let doomed: Option<String> = conn
        .pending_perm
        .lock()
        .unwrap()
        .iter()
        .find(|(_, (id, _))| id == rpc_id)
        .map(|(k, _)| k.clone());
    if let Some(request_id) = doomed {
        let _ = respond_permission_inner(conn, &request_id, None);
    }
}

fn reader_loop(app: AppHandle, conn: Arc<AcpConn>, stdout: std::process::ChildStdout) {
    for line in BufReader::new(stdout).lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        // probe §5：非 JSON 行静默忽略（opencode 偶发杂输出不影响协议流）
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        handle_incoming(&app, &conn, &msg);
    }
    mark_dead(&app, &conn, "stdout EOF（进程退出）");
    // 收尸，避免僵尸进程
    let _ = conn.child.lock().unwrap().wait();
}

fn mark_dead(app: &AppHandle, conn: &Arc<AcpConn>, reason: &str) {
    if conn.dead.swap(true, Ordering::SeqCst) {
        return;
    }
    // 清空 pending：等待方立即收到 Disconnected 失败，不挂满超时
    conn.pending.lock().unwrap().clear();
    // 权限请求也随进程失效：前端答复会走 require_conn 的 acp-dead 分支，这里清防泄漏
    conn.pending_perm.lock().unwrap().clear();
    let tail: Vec<String> = {
        let t = conn.stderr_tail.lock().unwrap();
        t.iter().rev().take(5).rev().cloned().collect()
    };
    let full_reason = if tail.is_empty() {
        reason.to_string()
    } else {
        format!("{}；stderr: {}", reason, tail.join(" | "))
    };
    *dead_reason_slot().lock().unwrap() = Some(full_reason.clone());
    {
        let mut slot = conn_slot().lock().unwrap();
        if let Some(cur) = slot.as_ref() {
            if Arc::ptr_eq(cur, conn) {
                *slot = None;
            }
        }
    }
    let _ = app.emit(
        "acp://status",
        json!({ "status": "dead", "reason": full_reason }),
    );
}

fn drain_stderr(stderr: std::process::ChildStderr, tail: Arc<Mutex<VecDeque<String>>>) {
    // 独立线程 drain 防管道满死锁（tauri-bridge spec §4）；仅保留尾部若干行供诊断
    for line in BufReader::new(stderr).lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let mut t = tail.lock().unwrap();
        if t.len() >= STDERR_KEEP_LINES {
            t.pop_front();
        }
        t.push_back(line);
    }
}

// ---------- 启动 / 清理 ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub name: String,
    pub version: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StartResult {
    pub status: String,
    pub protocol_version: u64,
    pub agent_info: Option<AgentInfo>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusResult {
    pub status: String,
    pub reason: Option<String>,
    pub agent_info: Option<AgentInfo>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResult {
    pub session_id: String,
    pub config_options: Value,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptResult {
    pub stop_reason: String,
    pub usage: Value,
}

fn agent_info_from(v: &Value) -> Option<AgentInfo> {
    Some(AgentInfo {
        name: v.get("name")?.as_str()?.to_string(),
        version: v.get("version")?.as_str()?.to_string(),
    })
}

fn start_inner(app: AppHandle) -> Result<StartResult, String> {
    // 串行化启动：并发 send（跨会话允许并发）同时调 acp_start 时，
    // 防止双 spawn 或在握手完成前放行后续请求（协议 §3：initialize 必须最先完成）
    static START_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _start_guard = START_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();

    // 幂等（design D1）：已有活进程直接复用，避免重复 8s 握手
    if let Some(conn) = current_conn() {
        if !conn.dead.load(Ordering::SeqCst) {
            let info = conn.agent_info.lock().unwrap().clone();
            return Ok(StartResult {
                status: "ready".into(),
                protocol_version: 1,
                agent_info: agent_info_from(&info),
            });
        }
    }
    *dead_reason_slot().lock().unwrap() = None;

    let mut cmd = match resolve_spawn() {
        SpawnSpec::Exe(path) => {
            let mut c = Command::new(path);
            c.arg("acp");
            c
        }
        SpawnSpec::CmdFallback => {
            let mut c = Command::new("cmd");
            c.arg("/C").arg("opencode").arg("acp");
            c
        }
    };
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    // ACP 自身 cwd 与会话 cwd 无关（session/new 传 cwd），无需 --cwd

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 opencode acp 失败: {}（未安装或 PATH 异常）", e))?;
    let stdin = child.stdin.take().ok_or("无法获取子进程 stdin")?;
    let stdout = child.stdout.take().ok_or("无法获取子进程 stdout")?;
    let stderr = child.stderr.take().ok_or("无法获取子进程 stderr")?;

    let stderr_tail: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));
    let conn = Arc::new(AcpConn {
        child: Mutex::new(child),
        stdin: Mutex::new(Some(BufWriter::new(stdin))),
        next_id: AtomicU64::new(1),
        pending: Mutex::new(HashMap::new()),
        pending_perm: Mutex::new(HashMap::new()),
        perm_next: AtomicU64::new(1),
        agent_info: Mutex::new(Value::Null),
        stderr_tail: stderr_tail.clone(),
        dead: AtomicBool::new(false),
    });
    *conn_slot().lock().unwrap() = Some(conn.clone());

    std::thread::spawn(move || drain_stderr(stderr, stderr_tail));
    {
        let reader_app = app.clone();
        let reader_conn = conn.clone();
        std::thread::spawn(move || reader_loop(reader_app, reader_conn, stdout));
    }

    // initialize 握手（10s 超时；D2：不声明 fs/terminal capability，缺省=不支持）
    let result = rpc_call(
        &conn,
        "initialize",
        json!({
            "protocolVersion": 1,
            "clientCapabilities": { "fs": { "readTextFile": false, "writeTextFile": false } },
            "clientInfo": { "name": "lightr-vibe", "version": "0.1.0" }
        }),
    )
    .inspect_err(|_| {
        cleanup_failed(&conn);
    })?;

    *conn.agent_info.lock().unwrap() = result
        .get("agentInfo")
        .cloned()
        .unwrap_or(Value::Null);
    let protocol_version = result
        .get("protocolVersion")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    let info = conn.agent_info.lock().unwrap().clone();
    Ok(StartResult {
        status: "ready".into(),
        protocol_version,
        agent_info: agent_info_from(&info),
    })
}

/// 握手失败清理：关 stdin + kill + 摘全局句柄
fn cleanup_failed(conn: &Arc<AcpConn>) {
    *conn.stdin.lock().unwrap() = None;
    let _ = conn.child.lock().unwrap().kill();
    let mut slot = conn_slot().lock().unwrap();
    if let Some(cur) = slot.as_ref() {
        if Arc::ptr_eq(cur, conn) {
            *slot = None;
        }
    }
}

/// 应用退出清理（lib.rs RunEvent::Exit）：关 stdin（probe §4：<1s rc=0）→
/// 等 2s → kill 兜底。
pub fn shutdown() {
    let Some(conn) = conn_slot().lock().unwrap().take() else {
        return;
    };
    *conn.stdin.lock().unwrap() = None;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match conn.child.lock().unwrap().try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = conn.child.lock().unwrap().kill();
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break,
        }
    }
}

// ---------- Tauri commands ----------

/// 启动（幂等）：解析真身路径 → spawn → initialize 握手 → ready
#[tauri::command]
pub async fn acp_start(app: AppHandle) -> Result<StartResult, String> {
    tauri::async_runtime::spawn_blocking(move || start_inner(app))
        .await
        .map_err(|e| format!("acp 任务执行失败: {}", e))?
}

#[tauri::command]
pub fn acp_status() -> StatusResult {
    if let Some(conn) = current_conn() {
        if !conn.dead.load(Ordering::SeqCst) {
            let info = conn.agent_info.lock().unwrap().clone();
            return StatusResult {
                status: "ready".into(),
                reason: None,
                agent_info: agent_info_from(&info),
            };
        }
    }
    let reason = dead_reason_slot().lock().unwrap().clone();
    StatusResult {
        status: if reason.is_some() { "dead" } else { "stopped" }.into(),
        reason,
        agent_info: None,
    }
}

fn parse_session_result(result: &Value, fallback_id: Option<String>) -> Result<SessionResult, String> {
    let session_id = result
        .get("sessionId")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
        .or(fallback_id)
        .ok_or("acp:-32603 会话响应缺少 sessionId")?;
    Ok(SessionResult {
        session_id,
        // opencode 走 configOptions 新机制（probe §2）；容忍缺失
        config_options: result.get("configOptions").cloned().unwrap_or(Value::Null),
    })
}

/// 新建 ACP 会话（首调 7.6-8.9s → 30s 超时；mcpServers 为必填字段，传 []）
#[tauri::command]
pub async fn acp_session_new(cwd: String) -> Result<SessionResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let conn = require_conn()?;
        // cwd 必须存在（协议 §9 绝对路径）；tmpsession 目录首次使用时自动创建
        std::fs::create_dir_all(&cwd).map_err(|e| format!("创建会话目录失败: {}", e))?;
        let result = rpc_call(&conn, "session/new", json!({ "cwd": cwd, "mcpServers": [] }))?;
        parse_session_result(&result, None)
    })
    .await
    .map_err(|e| format!("acp 任务执行失败: {}", e))?
}

/// 重载 ACP 会话（loadSession capability；历史以 session/update 在响应前重放，
/// 前端对无 handler 的重放 update 直接忽略——本地历史为准，design §4）
#[tauri::command]
pub async fn acp_session_load(
    acp_session_id: String,
    cwd: String,
) -> Result<SessionResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let conn = require_conn()?;
        let id = acp_session_id.clone();
        let result = rpc_call(
            &conn,
            "session/load",
            json!({ "sessionId": acp_session_id, "cwd": cwd, "mcpServers": [] }),
        )?;
        parse_session_result(&result, Some(id))
    })
    .await
    .map_err(|e| format!("acp 任务执行失败: {}", e))?
}

/// 发送 prompt（长请求：期间 session/update 流式推送，turn 结束才响应；300s 超时）
#[tauri::command]
pub async fn acp_prompt(session_id: String, text: String) -> Result<PromptResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let conn = require_conn()?;
        let result = rpc_call(
            &conn,
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": [{ "type": "text", "text": text }]
            }),
        )?;
        Ok(PromptResult {
            stop_reason: result
                .get("stopReason")
                .and_then(Value::as_str)
                .unwrap_or("end_turn")
                .to_string(),
            // opencode 扩展字段 usage（probe §3：inputTokens/outputTokens/totalTokens），原样透传
            usage: result.get("usage").cloned().unwrap_or(Value::Null),
        })
    })
    .await
    .map_err(|e| format!("acp 任务执行失败: {}", e))?
}

/// 停止 turn：session/cancel notification（无响应；idle 时为安全 no-op，probe §4）
/// + client 义务：该会话 pending 的 request_permission 一律回 cancelled（协议 §5）
#[tauri::command]
pub fn acp_cancel(session_id: String) -> Result<(), String> {
    let conn = require_conn()?;
    send_line(
        &conn,
        &json!({ "jsonrpc": "2.0", "method": "session/cancel", "params": { "sessionId": session_id } }),
    )?;
    let doomed: Vec<String> = conn
        .pending_perm
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, (_, sid))| *sid == session_id)
        .map(|(k, _)| k.clone())
        .collect();
    for request_id in doomed {
        let _ = respond_permission_inner(&conn, &request_id, None);
    }
    Ok(())
}

/// 切换会话配置（优先规范方法 set_config_option，probe §6）→ 返回全量 configOptions
#[tauri::command]
pub async fn acp_set_config(
    session_id: String,
    config_id: String,
    value: Value,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let conn = require_conn()?;
        rpc_call(
            &conn,
            "session/set_config_option",
            json!({ "sessionId": session_id, "configId": config_id, "value": value }),
        )
    })
    .await
    .map_err(|e| format!("acp 任务执行失败: {}", e))?
}

/// 答复权限请求；option_id 为 None = cancelled 语义（design §2，协议 §5）
#[tauri::command]
pub fn acp_respond_permission(
    request_id: String,
    option_id: Option<String>,
) -> Result<(), String> {
    let conn = require_conn()?;
    respond_permission_inner(&conn, &request_id, option_id.as_deref())
}

fn respond_permission_inner(
    conn: &AcpConn,
    request_id: &str,
    option_id: Option<&str>,
) -> Result<(), String> {
    let entry = conn.pending_perm.lock().unwrap().remove(request_id);
    let Some((rpc_id, _session_id)) = entry else {
        return Err(format!(
            "acp:-32002 权限请求不存在或已答复: {}",
            request_id
        ));
    };
    let outcome = match option_id {
        Some(o) => json!({ "outcome": { "outcome": "selected", "optionId": o } }),
        None => json!({ "outcome": { "outcome": "cancelled" } }),
    };
    send_line(
        conn,
        &json!({ "jsonrpc": "2.0", "id": rpc_id, "result": outcome }),
    )
}

// ---------- 单元测试（协议路由 / 超时矩阵 / pending 清理 / shim 解析） ----------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_routes_three_ways() {
        // response：有 id 无 method
        assert_eq!(
            classify(&json!({"jsonrpc":"2.0","id":2,"result":{"stopReason":"end_turn"}})),
            Route::Response(2)
        );
        // notification：有 method 无 id
        assert_eq!(
            classify(&json!({"jsonrpc":"2.0","method":"session/update","params":{}})),
            Route::Notification("session/update".into())
        );
        // agent→client request：两者都有
        assert_eq!(
            classify(&json!({"jsonrpc":"2.0","id":3,"method":"session/request_permission","params":{}})),
            Route::Request(json!(3), "session/request_permission".into())
        );
        // 字符串 id 的 request 也必须原样保留（响应回显）
        assert_eq!(
            classify(&json!({"jsonrpc":"2.0","id":"abc","method":"session/request_permission","params":{}})),
            Route::Request(json!("abc"), "session/request_permission".into())
        );
        assert_eq!(classify(&json!({"jsonrpc":"2.0"})), Route::Unknown);
        // id:null 的异常消息按 Unknown 丢弃，不误路由为 response
        assert_eq!(classify(&json!({"jsonrpc":"2.0","id":null,"result":{}})), Route::Unknown);
    }

    #[test]
    fn non_json_line_is_ignorable() {
        // probe §5：杂输出行解析失败 → 读线程 continue，不崩溃
        assert!(serde_json::from_str::<Value>("gibberish").is_err());
        assert!(serde_json::from_str::<Value>("").is_err());
    }

    #[test]
    fn timeout_matrix_matches_prd() {
        assert_eq!(timeout_for("initialize"), Duration::from_secs(10));
        assert_eq!(timeout_for("session/new"), Duration::from_secs(30));
        assert_eq!(timeout_for("session/load"), Duration::from_secs(30));
        assert_eq!(timeout_for("session/prompt"), Duration::from_secs(300));
        assert_eq!(
            timeout_for("session/set_config_option"),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn rpc_error_string_keeps_code() {
        let err = json!({"code":-32602,"message":"Invalid params: session not found: ses_bogus"});
        assert_eq!(
            rpc_error_string(&err),
            "acp:-32602 Invalid params: session not found: ses_bogus"
        );
        // 缺字段兜底
        assert_eq!(rpc_error_string(&json!({})), "acp:-32603 unknown error");
    }

    #[test]
    fn pending_registered_and_taken_once() {
        let mut map: HashMap<u64, Sender<Value>> = HashMap::new();
        let rx = register_pending(&mut map, 7);
        assert!(map.contains_key(&7));
        let tx = take_pending(&mut map, 7).expect("registered");
        tx.send(json!({"ok":true})).unwrap();
        assert_eq!(rx.recv().unwrap(), json!({"ok":true}));
        // 已清理：迟到响应到达时按 id 查不到即被丢弃
        assert!(take_pending(&mut map, 7).is_none());
    }

    #[test]
    fn pending_timeout_cleanup_does_not_hang() {
        let mut map: HashMap<u64, Sender<Value>> = HashMap::new();
        let rx = register_pending(&mut map, 9);
        // 模拟 rpc_call 超时路径：先移除 pending；sender 丢弃后 recv 立即失败
        take_pending(&mut map, 9);
        assert!(matches!(
            rx.recv_timeout(Duration::from_millis(20)),
            Err(RecvTimeoutError::Disconnected)
        ));
        assert!(map.is_empty());
    }

    #[test]
    fn shim_exe_extraction_version_fox() {
        // 实测 shim 形态（probe §0）
        let shim = "@echo off\r\n\"%dp0%\\node_modules\\opencode-ai\\bin\\opencode.exe\" %*\r\n";
        assert_eq!(
            extract_exe_from_shim(shim, "C:\\Users\\u\\.version-fox\\sdks\\nodejs\\"),
            Some(
                "C:\\Users\\u\\.version-fox\\sdks\\nodejs\\node_modules\\opencode-ai\\bin\\opencode.exe"
                    .to_string()
            )
        );
        // %~dp0 变体（自带分隔符语义）
        let shim2 = "\"%~dp0node_modules\\opencode-ai\\bin\\opencode.exe\" %*";
        assert_eq!(
            extract_exe_from_shim(shim2, "C:\\shimdir"),
            Some("C:\\shimdir\\node_modules\\opencode-ai\\bin\\opencode.exe".to_string())
        );
        // 绝对路径 token：原样返回
        let shim3 = "\"D:\\tools\\opencode.exe\" %*";
        assert_eq!(
            extract_exe_from_shim(shim3, "C:\\shimdir"),
            Some("D:\\tools\\opencode.exe".to_string())
        );
        // 无 exe token → None（走 cmd /C 末选）
        assert_eq!(extract_exe_from_shim("echo hello", "C:\\d"), None);
    }
}
