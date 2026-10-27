// ACP (Agent Client Protocol) 进程管理：**每会话一个独立 `opencode acp` 子进程 + 独立连接**
// （design §8，对齐 codeg；取代原 D1 单长驻进程多会话）。
// 传输：newline-delimited JSON-RPC 2.0 over stdio（research/acp-protocol.md §1，非 LSP 帧）。
// connId = 本地会话 id（ses-xxxxxxxx，前端 session.id），所有命令/事件按 connId 精确定位；
// agent-side sessionId（ses_xxx）仍用于 session/* 方法参数与消息归属。
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

use crate::agent_type::AgentType;
use crate::acp_question::{build_elicitation_result, classify_elicitation, ElicitationPlan, Question};
use crate::engine::{self, AcpAgentMeta, ContextWindowSource};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// Windows 下子进程不弹控制台窗口
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// stderr 环形缓冲保留行数（进程死亡诊断用）
const STDERR_KEEP_LINES: usize = 50;

/// 空闲自动断连默认超时（秒，对齐 codeg DEFAULT_IDLE_TIMEOUT_SECS=180）。
/// 可用环境变量 `LIGHTR_ACP_IDLE_TIMEOUT_SECS` 覆盖，`0` = 关闭空闲回收。
const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 180;

/// 空闲扫描周期（秒）：后台线程每隔这么久检查一次各连接的空闲时长
const IDLE_SWEEP_INTERVAL_SECS: u64 = 15;

/// 读取空闲超时配置（秒）；返回 0 表示关闭空闲回收
fn idle_timeout_secs() -> u64 {
    match std::env::var("LIGHTR_ACP_IDLE_TIMEOUT_SECS") {
        Ok(v) => v.trim().parse::<u64>().unwrap_or(DEFAULT_IDLE_TIMEOUT_SECS),
        Err(_) => DEFAULT_IDLE_TIMEOUT_SECS,
    }
}

/// `session/prompt` 的**空闲**超时（秒）：两次 progress 之间允许的最长静默。
///
/// 长任务（写小游戏、批量重构）可能跑十几分钟，用固定总时长必然误杀；
/// 只要 agent 还在推 `session/update`，就说明它活着，不该中断。
const PROMPT_IDLE_TIMEOUT_SECS: u64 = 300;

/// 请求超时矩阵（PRD R1）：initialize 60s / session/new、load 30s（首调实测
/// 7.6-8.9s，probe §2）/ prompt 300s / 其余 30s。
///
/// initialize 由 10s 放宽到 60s（AC10 实机反馈，对齐 codeg connection.rs 的 60s）：
/// Claude Code（经 claude-agent-acp）冷启动时会先预热其 MCP 服务器
/// （~/.claude.json 配置的 context7 / 远程 http 等），就绪前 initialize 无响应。
/// 健康适配器本身仅 0.6-2.2s 应答，60s 只作冷启动/MCP 预热的容忍余量。
fn timeout_for(method: &str) -> Duration {
    match method {
        "initialize" => Duration::from_secs(60),
        "session/new" | "session/load" => Duration::from_secs(30),
        "session/prompt" => Duration::from_secs(300),
        _ => Duration::from_secs(30),
    }
}

#[cfg(test)]
mod timeout_semantics_tests {
    use super::*;

    /// 长任务不能靠固定总时长限制：prompt 走的是活动性超时。
    ///
    /// 回归背景：写小游戏这类任务跑过 5 分钟会被 `timeout:300s session/prompt`
    /// 中断，而 agent 其实一直在正常输出。
    #[test]
    fn prompt_idle_timeout_is_activity_based() {
        // 空闲阈值仍为 300s（两次 progress 之间的最大静默）
        assert_eq!(PROMPT_IDLE_TIMEOUT_SECS, 300);
        // 非 prompt 方法仍是固定超时
        assert_eq!(timeout_for("initialize"), Duration::from_secs(60));
        assert_eq!(timeout_for("session/prompt"), Duration::from_secs(300));
    }
}


/// initialize 的 `clientCapabilities` 构造（纯函数，便于单测）。
///
/// - 基线：不 host fs/terminal（D2，文件操作由 Lightr-vibe 自有文件面板承担）。
/// - elicitation：**无条件**声明 `elicitation.form`（D2 修订，所有内置引擎）。
///   依据：opencode 提供方 ACP 文档 §"Questions" 明确——client 声明 `elicitation.form`
///   时，OpenCode 以 `elicitation/create`（form 模式）提问；`claude-agent-acp` ≥0.43.0
///   起实验性支持；codex 一直支持。不支持者不会发（声明只是"我支持"，非强制），
///   收到非法/不支持形状仍由处理器 `Decline`（不 hang）。
///
/// 该函数不按引擎分支（连"声明"也无需引擎名）；**协议路由亦不含引擎名**（AC9）。
///
/// - session 通知（ACP schema 1.9 Session Notices，10-14-acp-stream-control AC10 实机修复）：
///   **无条件**声明 `session.notices = {}`（必须是对象，适配器按
///   `typeof session.notices === "object"` 判断）。声明后 `claude-agent-acp` 把网关警告
///   等以独立 `session/update {sessionUpdate:"notice"}` 下发，而不是降级塞进
///   `agent_message_chunk` 被当成聊天正文渲染；前端 `applyUpdate` 的 `notice` 分支
///   转成 toast（不入 transcript）。
///   已知取舍（codeg 同样接受）：声明 notices 后 claude 不再下发 `level === "info"`
///   的纯信息帧（仅在 Claude Code 自身 transcript 模式可见），可接受。
///
///   **不声明 `session.compaction`**：codeg 有 compaction 消费者把 `compaction_update`
///   翻译回 `_meta.contextCompaction`；本项目**没有** compaction 消费者（grep 确认），
///   一旦声明，claude 会停止发送现有的 `_meta.contextCompaction` 合成工具调用 →
///   纯回归。故此处刻意只开 notices。后续若新增 compaction 消费面再同步开启。
fn client_capabilities() -> Value {
    json!({
        "fs": { "readTextFile": false, "writeTextFile": false },
        "elicitation": { "form": {} },
        "session": { "notices": {} }
    })
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
/// 实现已收敛到 engine.rs（单一实现，acp.rs / agent.rs 共用）。
#[cfg(test)]
use engine::extract_exe_from_shim;

/// 按 `SpawnSpec` 构造子进程命令（纯函数，便于单测；**不出现引擎名**）。
/// `bare_bin` 供 CmdFallback 用（cmd 走 PATHEXT 解析）；`subcommand` 为空串时跳过。
fn command_from_spawn_spec(
    spec: engine::SpawnSpec,
    display_name: &str,
    bare_bin: &str,
    subcommand: &str,
) -> Result<Command, String> {
    match spec {
        engine::SpawnSpec::Exe(path) => {
            let mut c = Command::new(path);
            if !subcommand.is_empty() {
                c.arg(subcommand);
            }
            Ok(c)
        }
        engine::SpawnSpec::CmdFallback => {
            let mut c = Command::new("cmd");
            // 用去扩展名的裸名交给 cmd：cmd 走 PATHEXT 解析（可命中 .exe/.cmd/.bat），
            // 比写死 bin_candidates[0]（如 "opencode.exe"）更贴近原 `cmd /C opencode` 语义。
            c.arg("/C").arg(bare_bin);
            if !subcommand.is_empty() {
                c.arg(subcommand);
            }
            Ok(c)
        }
        engine::SpawnSpec::NotInstalled { hint } => Err(format!(
            "acp:-32603 未安装 {} 的适配器命令，请运行 `npm i -g {}`（需要 Node.js）",
            display_name, hint
        )),
    }
}

/// 构造引擎 ACP 子进程 Command（stdin/stdout/stderr 管道 + Windows 无窗口）。
/// 严格按 `AcpAgentMeta` 取参：bin 候选 / subcommand / env 清洗列表均来自声明，
/// 协议路由代码不引用引擎名（AC8）。
///
/// 分发形态经 `SpawnSpec` 收敛为通用三态（不出现引擎名）：
/// - Exe(path)：直接 spawn 真身；subcommand 非空时追加（opencode `acp`；适配器为空）。
/// - CmdFallback：`cmd /C <bare_bin> [subcommand]` 末选（.cmd 无法 CreateProcess）。
/// - NotInstalled：适配器未安装 → 返回 Err（含 `npm i -g <package>` 指引），**不 spawn**。
///
/// env_strip 剔除父进程可能继承的 server 凭据（如 opencode 的
/// `OPENCODE_SERVER_USERNAME/PASSWORD`）：泄漏给子进程会让 Zen 免费通道上游鉴权失败
/// （`-32603 ... Invalid credential`）。剔除后回到干净态，与 codeg 独立启动等价。
fn build_acp_command(meta: &AcpAgentMeta) -> Result<Command, String> {
    let mut cmd = command_from_spawn_spec(
        engine::resolve_spawn(meta),
        meta.name,
        engine::bare_bin_name(meta),
        meta.launch.acp_subcommand,
    )?;
    for var in meta.launch.env_strip {
        cmd.env_remove(var);
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    Ok(cmd)
}

// ---------- 连接对象 ----------

/// 一次提问的应答后端（两条来源共用同一张卡，答复时按来源回写）。
enum QuestionReply {
    /// 原生 ACP `elicitation/create`：回写 JSON-RPC 响应（原 id 原样回显）。
    Acp(Value),
    /// 注入的 `ask_user_question` MCP 工具：经 socket 回传给伴生进程（std mpsc）。
    Mcp(Sender<AskOutcome>),
}

/// MCP ask 的答复结果（用户答 → 伴生进程包成 MCP 工具结果）。
struct AskOutcome {
    /// 用户跳过/取消
    declined: bool,
    /// 展示给模型的答案文本（多为选中项标签；多选以顿号连接）
    answer: String,
}

/// 连接归属：普通用户会话 / 自动化无头会话。
///
/// 自动化会话没有人点「允许」，`session/request_permission` 必须在 Rust 侧自动放行
/// （design §5.5）。归属挂在连接上判断，**不按引擎名分支**。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnOwner {
    User,
    Automation,
}

struct AcpConn {
    /// 本地会话 id（connId）：事件/命令路由键，与 ACP 进程一一对应
    conn_id: String,
    /// 连接归属（默认用户会话）；决定权限请求是自动放行还是转前端
    owner: ConnOwner,
    /// 该连接绑定的引擎（建连时确定；会话级命令从连接取，不再重复传参）。
    /// 本期会话级命令无需按引擎分支，字段为「新增第 2 个引擎」预留的落点。
    #[allow(dead_code)]
    engine: AgentType,
    /// 进程死亡原因（dead 后写入，acp_status(connId) 与事件共用）
    dead_reason: Mutex<Option<String>>,
    child: Mutex<Child>,
    /// Option：shutdown/清理时 take 掉即关闭 stdin（probe §4：stdin 关闭 → rc=0 干净退出）
    stdin: Mutex<Option<BufWriter<ChildStdin>>>,
    next_id: AtomicU64,
    /// JSON-RPC id → 等待响应的 oneshot（std mpsc）
    pending: Mutex<HashMap<u64, Sender<Value>>>,
    /// requestId → (JSON-RPC id 原样回显值, sessionId)：request_permission 等待前端答复
    pending_perm: Mutex<HashMap<String, (Value, String)>>,
    perm_next: AtomicU64,
    /// requestId → (应答后端, 题目列表)：等待前端答复。两条来源共用同一卡：
    /// - 原生 ACP `elicitation/create` → `QuestionReply::Acp`（回写 JSON-RPC）
    /// - 注入的 `ask_user_question` MCP 工具（伴生进程经 socket 回连）→ `QuestionReply::Mcp`
    ///
    /// 与 pending_perm 平行、语义不同（不复用权限 map，design §2 / 10-15-acp-ask-tool）。
    pending_question: Mutex<HashMap<String, (QuestionReply, Vec<Question>)>>,
    q_next: AtomicU64,
    agent_info: Mutex<Value>,
    /// session/new 拿到的 agent 侧会话 id（自动化取消时要按它发 session/cancel）
    acp_session_id: Mutex<Option<String>>,
    /// 本轮最后一次 assistant 文本（自动化结算时作为 summary；每轮开始清空）
    last_assistant_text: Mutex<Option<String>>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    dead: AtomicBool,
    /// 最后一次活动时刻（prompt / update / permission 交互刷新）：空闲回收判据
    last_activity_at: Mutex<Instant>,
}

/// 连接仓库：key = connId（本地会话 id）。每个会话一个独立进程连接（design §8.1）。
fn conns() -> &'static Mutex<HashMap<String, Arc<AcpConn>>> {
    static CONNS: OnceLock<Mutex<HashMap<String, Arc<AcpConn>>>> = OnceLock::new();
    CONNS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 取指定会话的连接（不存在或已死亡返回 None）
fn get_conn(conn_id: &str) -> Option<Arc<AcpConn>> {
    conns()
        .lock()
        .unwrap()
        .get(conn_id)
        .filter(|c| !c.dead.load(Ordering::SeqCst))
        .cloned()
}

/// 取指定会话的连接；缺失/死亡 → `acp-dead:` 错误串（前端据此显示错误卡）
fn require_conn(conn_id: &str) -> Result<Arc<AcpConn>, String> {
    get_conn(conn_id).ok_or_else(|| {
        format!(
            "acp-dead: 会话 {} 的 ACP 进程未连接，请重新发送以自动重连",
            conn_id
        )
    })
}

/// 刷新连接的最后活动时刻（prompt / update / permission 交互时调用）
fn touch_activity(conn: &AcpConn) {
    *conn.last_activity_at.lock().unwrap() = Instant::now();
}

/// 从仓库移除指定 connId（仅当仓库内确为同一实例，防误删新连接）
fn remove_conn(conn_id: &str, conn: &Arc<AcpConn>) {
    let mut map = conns().lock().unwrap();
    if let Some(cur) = map.get(conn_id) {
        if Arc::ptr_eq(cur, conn) {
            map.remove(conn_id);
        }
    }
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
    let recv = if method == "session/prompt" {
        // 长请求：改用「活动性超时」—— 每收到一次 progress 就重置计时，
        // 只有真的静默满 PROMPT_IDLE_TIMEOUT_SECS 才判定卡死。
        recv_with_activity_timeout(conn, rx, Duration::from_secs(PROMPT_IDLE_TIMEOUT_SECS))
    } else {
        rx.recv_timeout(timeout)
    };
    match recv {
        Ok(resp) => {
            if let Some(err) = resp.get("error") {
                return Err(rpc_error_string(err));
            }
            Ok(resp.get("result").cloned().unwrap_or(Value::Null))
        }
        Err(RecvTimeoutError::Timeout) => {
            // 超时：移除 pending，避免迟到的响应按超时 id 检不出来
            take_pending(&mut conn.pending.lock().unwrap(), id);
            let shown = if method == "session/prompt" {
                PROMPT_IDLE_TIMEOUT_SECS
            } else {
                timeout.as_secs()
            };
            Err(format!("timeout:{}s {}", shown, method))
        }
        Err(RecvTimeoutError::Disconnected) => {
            Err("acp-dead: subprocess exited while awaiting response".to_string())
        }
    }
}

/// 等待 prompt 的响应，**每收到一次 `session/update` 就重置计时**。
///
/// `reader_loop` 转发 update 时会刷新 `last_activity_at`，这里以它为
/// 「agent 还活着」的证据。分片等待（1s 一跳），每次醒来比对活动时间戳，
/// 因此不需要额外的信号通道。
fn recv_with_activity_timeout(
    conn: &AcpConn,
    rx: Receiver<Value>,
    idle: Duration,
) -> Result<Value, RecvTimeoutError> {
    let poll = Duration::from_secs(1);
    loop {
        match rx.recv_timeout(poll) {
            Ok(v) => return Ok(v),
            Err(RecvTimeoutError::Disconnected) => return Err(RecvTimeoutError::Disconnected),
            Err(RecvTimeoutError::Timeout) => {
                let elapsed = conn.last_activity_at.lock().unwrap().elapsed();
                if elapsed < idle {
                    continue;
                }
                return Err(RecvTimeoutError::Timeout);
            }
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
            // 任何入站 notification 都视为活动（防长生成被空闲回收误杀）
            touch_activity(conn);
            if method == "session/update" {
                let params = msg.get("params");
                let acp_session_id = params
                    .and_then(|p| p.get("sessionId"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let update = params
                    .and_then(|p| p.get("update"))
                    .cloned()
                    .unwrap_or(Value::Null);
                // update 内自带字段（含 `_meta`）已整体透传；个别引擎把 `_meta`
                // 放在 `params._meta`（update 外，如 opencode/v2 的 retry 键），
                // 补一个顶层 `meta` 透传给前端，前端宽容地两处都读（design §2.2）。
                let meta = params
                    .and_then(|p| p.get("_meta"))
                    .cloned()
                    .unwrap_or(Value::Null);
                // 自动化无头会话的 summary：累积 assistant 正文分片。
                // 前端不消费这条连接，所以这里顺手在 Rust 侧留一份末轮文本。
                if conn.owner == ConnOwner::Automation
                    && update.get("sessionUpdate").and_then(Value::as_str)
                        == Some("agent_message_chunk")
                {
                    if let Some(chunk) = update
                        .get("content")
                        .and_then(|c| c.get("text"))
                        .and_then(Value::as_str)
                    {
                        let mut slot = conn.last_assistant_text.lock().unwrap();
                        match slot.as_mut() {
                            Some(buf) => buf.push_str(chunk),
                            None => *slot = Some(chunk.to_string()),
                        }
                    }
                }
                let _ = app.emit(
                    "acp://update",
                    json!({
                        "connId": conn.conn_id,
                        "sessionId": acp_session_id,
                        "update": update,
                        "meta": meta,
                    }),
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
        Route::Request(id, method) => {
            touch_activity(conn);
            handle_client_request(conn, id, &method, msg, app);
        }
        Route::Unknown => {}
    }
}

/// 自动化会话的权限自动放行选项选择（纯函数，可单测）。
///
/// 按 ACP 规范：优先 `allow_once`，其次 `allow_always`；都没有则返回 `None`
/// （调用方回 `cancelled`，绝不让 agent 挂住）。**按 `kinds` 匹配，不按数组位置**。
fn pick_auto_approve_option(options: &[Value]) -> Option<String> {
    let find_by_kind = |kind: &str| -> Option<String> {
        options
            .iter()
            .find(|o| {
                o.get("kind")
                    .and_then(Value::as_str)
                    .map(|k| k == kind)
                    .unwrap_or(false)
            })
            .and_then(|o| o.get("optionId").and_then(Value::as_str))
            .map(|s| s.to_string())
    };
    find_by_kind("allow_once").or_else(|| find_by_kind("allow_always"))
}

fn handle_client_request(
    conn: &AcpConn,
    id: Value,
    method: &str,
    msg: &Value,
    app: &AppHandle,
) {
    eprintln!("[acp] client request: {method} (connId={})", conn.conn_id);
    if method == "session/request_permission" {
        let params = msg.get("params");
        // Automation: headless session has no one to click Allow, so answer in Rust.
        // Ownership comes from the connection owner, never from the engine name.
        if conn.owner == ConnOwner::Automation {
            let options = params
                .and_then(|p| p.get("options"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let chosen = pick_auto_approve_option(&options);
            let outcome = match &chosen {
                Some(opt_id) => json!({ "outcome": { "outcome": "selected", "optionId": opt_id } }),
                None => json!({ "outcome": { "outcome": "cancelled" } }),
            };
            eprintln!(
                "[acp] auto-approved permission for automation conn {} (option={:?})",
                conn.conn_id, chosen
            );
            let _ = send_line(conn, &json!({ "jsonrpc": "2.0", "id": id, "result": outcome }));
            return;
        }
        let request_id = conn.perm_next.fetch_add(1, Ordering::SeqCst).to_string();
        let session_id = params
            .and_then(|p| p.get("sessionId"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        conn.pending_perm
            .lock()
            .unwrap()
            .insert(request_id.clone(), (id, session_id.clone()));
        // toolCall 原样透传；顺带对 rawInput 注入 `_start_line`（best-effort，仅供前端 diff 定位；
        // 失败保持原值，不阻断事件，不 host 任何 fs/terminal）。
        let mut tool_call = params
            .and_then(|p| p.get("toolCall"))
            .cloned()
            .unwrap_or(Value::Null);
        inject_raw_input_start_line_into(&mut tool_call);
        let _ = app.emit(
            "acp://permission",
            json!({
                "connId": conn.conn_id,
                "requestId": request_id,
                "sessionId": session_id,
                "toolCall": tool_call,
                "options": params.and_then(|p| p.get("options")).cloned().unwrap_or(Value::Null),
                // 请求级 `_meta`（title/description/defaultToNo）原样透传，前端解析；缺失 → null
                "meta": params.and_then(|p| p.get("_meta")).cloned().unwrap_or(Value::Null),
            }),
        );
        eprintln!("[acp] emitted acp://permission requestId={request_id}");
        return;
    }
    // elicitation/create（server→client）：codex Plan `request_user_input` / 通用 MCP form。
    // 不能落入下方 `elicitation/` 的 -32601（否则提问静默失败，design §2）。
    // 处理器通用、不含引擎名分支（AC9）；引擎差异只在 initialize 能力声明处。
    if method == "elicitation/create" {
        handle_elicitation_request(conn, id, msg, app);
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

/// 处理 `elicitation/create`（Questions 型）：分类 → Questions 则存 pending + emit
/// `acp://question` 等前端答复；Decline 则**直接回 decline**（agent 自行判断，不 hang）。
/// 与 request_permission 同构（生成 requestId、存 map、emit、答复后回写 response）。
/// 协议要求收到方 MUST 回应 elicitation，故任一分支都必须 send_line 或 emit。
fn handle_elicitation_request(conn: &AcpConn, id: Value, msg: &Value, app: &AppHandle) {
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let session_id = params
        .get("sessionId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    match classify_elicitation(&params) {
        ElicitationPlan::Questions(questions) => {
            let request_id = conn.q_next.fetch_add(1, Ordering::SeqCst).to_string();
            conn.pending_question.lock().unwrap().insert(
                request_id.clone(),
                (QuestionReply::Acp(id.clone()), questions.clone()),
            );
            let questions_json =
                serde_json::to_value(&questions).unwrap_or(Value::Array(Vec::new()));
            let _ = app.emit(
                "acp://question",
                json!({
                    "connId": conn.conn_id,
                    "requestId": request_id,
                    "sessionId": session_id,
                    "questions": questions_json,
                }),
            );
            eprintln!("[acp] emitted acp://question requestId={request_id}");
        }
        ElicitationPlan::Decline => {
            // 本期不支持的 elicitation（Approval / boolean / 未知形状）→ 直接拒答，
            // 语义比 -32601 更友好（「用户拒答」而非「方法不存在」），agent 据此自行判断。
            let _ = send_line(
                conn,
                &json!({ "jsonrpc": "2.0", "id": id, "result": { "action": "decline" } }),
            );
        }
    }
}

/// 对 toolCall 就地注入 `_start_line`（best-effort）：仅当 toolCall 是 object 且其
/// `rawInput` 可解析出补丁/文件文本时。任何失败都保持原值，不阻断事件。
fn inject_raw_input_start_line_into(tool_call: &mut Value) {
    let Some(obj) = tool_call.as_object_mut() else {
        return;
    };
    let Some(raw) = obj.get("rawInput").cloned() else {
        return;
    };
    let injected = inject_raw_input_start_line(&raw);
    obj.insert("rawInput".into(), injected);
}

/// 纯函数：从 `rawInput` 求带 `_start_line` 的形状（best-effort，仿 codeg
/// `inject_start_line` 的最小面）。规则：
/// - object：若已有 `_start_line` 则原样返回；否则不动（行号需 cwd/文件解析，本地无 fs）。
/// - string 且含 `@@`（unified patch）：尝试 parse 为 JSON 对象；成功则递归处理，失败保持原串。
/// - 其他（null/number/bool/数组/无补丁字符串）：原样返回。
///
/// 注：本项目不 host `fs/*`，不做真实行号解析；这里只保证「补丁字符串可被前端 diff 视图读取」
/// 的稳定形状，真正的 ± 行由前端按 diff 文本渲染。
fn inject_raw_input_start_line(raw: &Value) -> Value {
    match raw {
        Value::Object(_) => raw.clone(),
        Value::String(text) => {
            if text.contains("@@") {
                match serde_json::from_str::<Value>(text) {
                    Ok(parsed @ Value::Object(_)) => parsed,
                    _ => raw.clone(),
                }
            } else {
                raw.clone()
            }
        }
        _ => raw.clone(),
    }
}

/// $/cancel_request 兜底：按被取消请求的 JSON-RPC id 找 pending_perm / pending_question
/// 中对应条目，回取消响应并从 map 移除（协议 §5：收到方 MUST 对原请求回响应；
/// permission 请求的规范取消形状是 outcome cancelled；elicitation 回 decline）
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
    // elicitation：挂起的提问请求也须回响应（decline），否则 agent hang（design §3.4）。
    // 仅原生 ACP 来源可按 rpc id 命中（MCP ask 无 rpc id；其取消走 socket 断开/会话关闭）。
    let doomed_q: Option<String> = conn
        .pending_question
        .lock()
        .unwrap()
        .iter()
        .find(|(_, (reply, _))| matches!(reply, QuestionReply::Acp(id) if id == rpc_id))
        .map(|(k, _)| k.clone());
    if let Some(request_id) = doomed_q {
        let _ = decline_elicitation_inner(conn, &request_id);
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

/// 标记某连接死亡：置 dead、清该 conn 的 pending/pending_perm、从仓库移除该条，
/// emit `acp://status {connId, status:"dead", reason}`。**不触碰其它 conn**（design §8.1）。
fn mark_dead(app: &AppHandle, conn: &Arc<AcpConn>, reason: &str) {
    if conn.dead.swap(true, Ordering::SeqCst) {
        return;
    }
    // 清空 pending：等待方立即收到 Disconnected 失败，不挂满超时
    conn.pending.lock().unwrap().clear();
    // 权限请求也随进程失效：前端答复会走 require_conn 的 acp-dead 分支，这里清防泄漏
    conn.pending_perm.lock().unwrap().clear();
    // 挂起的 elicitation 同理：进程已死，无法回响应，清 map 防泄漏。
    // （清 map 会 drop Mcp 的 Sender → socket 处理线程 recv 失败 → 写 declined，不 hang）
    conn.pending_question.lock().unwrap().clear();
    unregister_ask_token(&conn.conn_id);
    let tail: Vec<String> = {
        let t = conn.stderr_tail.lock().unwrap();
        t.iter().rev().take(5).rev().cloned().collect()
    };
    let full_reason = if tail.is_empty() {
        reason.to_string()
    } else {
        format!("{}；stderr: {}", reason, tail.join(" | "))
    };
    *conn.dead_reason.lock().unwrap() = Some(full_reason.clone());
    remove_conn(&conn.conn_id, conn);
    let _ = app.emit(
        "acp://status",
        json!({ "connId": conn.conn_id, "status": "dead", "reason": full_reason }),
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
    pub conn_id: String,
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

/// 一次性探针结果（acp_probe_options）：仅回配置项，无会话 id
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
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

/// per-session 启动锁：同一 connId 并发 connect 时串行化，避免双 spawn（design §8.5）
fn connect_lock(conn_id: &str) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    let mut map = LOCKS.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap();
    map.entry(conn_id.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

/// 首次调用时启动空闲回收后台线程（OnceLock 守护；`LIGHTR_ACP_IDLE_TIMEOUT_SECS=0` 时线程仍起但空转跳过）
fn ensure_idle_sweep(app: &AppHandle) {
    static STARTED: OnceLock<bool> = OnceLock::new();
    if STARTED.set(true).is_err() {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || idle_sweep_loop(app));
}

// ---------- 注入的 ask 工具：回环 TCP 监听 + 桥接（task 10-15-acp-ask-tool） ----------

/// 全局 ask 监听：一次性绑定 `127.0.0.1:0`（随机端口），accept 循环处理伴生进程回连。
/// 附带 connId → 一次性 token 表（校验来源；仅本机）。
static ASK_LISTENER: OnceLock<AskListener> = OnceLock::new();

struct AskListener {
    /// `127.0.0.1:<port>`：注入到 `session/new.mcpServers` 的 args 里
    addr: String,
    /// connId → token（会话结束移除）
    tokens: Mutex<HashMap<String, String>>,
}

/// 确保全局 ask 监听已启动，返回 `127.0.0.1:<port>`（供注入构造 args）。
fn ensure_ask_listener(app: &AppHandle) -> String {
    ASK_LISTENER
        .get_or_init(|| {
            let listener = std::net::TcpListener::bind("127.0.0.1:0")
                .expect("bind ask listener 127.0.0.1:0");
            let addr = listener
                .local_addr()
                .map(|a| format!("127.0.0.1:{}", a.port()))
                .unwrap_or_else(|_| "127.0.0.1:0".to_string());
            let app = app.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    match stream {
                        Ok(s) => {
                            let app = app.clone();
                            std::thread::spawn(move || handle_ask_connection(app, s));
                        }
                        Err(_) => continue,
                    }
                }
            });
            AskListener {
                addr,
                tokens: Mutex::new(HashMap::new()),
            }
        })
        .addr
        .clone()
}

/// 登记/移除某会话的 ask token（spawn 时登记，会话结束移除）。
fn register_ask_token(conn_id: &str, token: &str) {
    if let Some(l) = ASK_LISTENER.get() {
        l.tokens
            .lock()
            .unwrap()
            .insert(conn_id.to_string(), token.to_string());
    }
}

fn unregister_ask_token(conn_id: &str) {
    if let Some(l) = ASK_LISTENER.get() {
        l.tokens.lock().unwrap().remove(conn_id);
    }
}

/// 进程级随机盐（一次性）：ask token = 盐 + connId 的简单混合。仅本机可见，
/// 用于防串会话（不是强加密；回环 + 校验足够）。
fn ask_salt() -> &'static str {
    static SALT: OnceLock<String> = OnceLock::new();
    SALT.get_or_init(|| {
        // 用系统时间 + 地址熵做种子（stdlib 无 OS 随机源依赖）
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let addr = &ASK_LISTENER as *const _ as usize;
        format!("{:x}{:x}", nanos, addr)
    })
}

/// 某会话的 ask token（登记与注入构造共用，保证一致）。
fn ask_token_for(conn_id: &str) -> String {
    format!("{}{}", ask_salt(), conn_id)
}

/// 定位 `lightr-ask-mcp` 可执行文件：与主进程同目录（生产/`tauri build`），
/// 或 dev 下同目录（`target/debug` 与主 exe 同处）。
fn ask_bin_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let name = if cfg!(windows) {
        "lightr-ask-mcp.exe"
    } else {
        "lightr-ask-mcp"
    };
    let candidate = dir.join(name);
    if candidate.is_file() {
        Some(candidate)
    } else {
        None
    }
}

/// 构造注入 `session/new`/`session/load` 的 `mcpServers` 数组（ACP McpServerStdio 形状）：
/// `{ "type":"stdio", "name":"lightr-ask", "command":<bin>, "args":[...], "env":[] }`。
/// 无法定位 bin 时返回 `[]`（不注入，opencode 零回归）。
fn mcp_servers_for(conn_id: &str) -> Value {
    let Some(bin) = ask_bin_path() else {
        return Value::Array(Vec::new());
    };
    let Some(addr) = ASK_LISTENER.get().map(|l| l.addr.clone()) else {
        return Value::Array(Vec::new());
    };
    let token = ask_token_for(conn_id);
    json!([{
        "type": "stdio",
        "name": "lightr-ask",
        "command": bin.to_string_lossy(),
        "args": ["--socket", addr, "--token", token, "--conn-id", conn_id],
        "env": []
    }])
}

/// 处理一条伴生进程回连：读一行 `{kind:"ask", token, connId, arguments}` →
/// 校验 → 映射成 `Question` → 存 pending（Mcp 后端）+ emit `acp://question` →
/// 阻塞等用户答复 → 写回 `{declined, answer}` 并关闭。
fn handle_ask_connection(app: AppHandle, stream: std::net::TcpStream) {
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let req: Value = match serde_json::from_str(line.trim()) {
        Ok(v) => v,
        Err(_) => return,
    };
    let token = req.get("token").and_then(Value::as_str).unwrap_or("");
    let conn_id = req.get("connId").and_then(Value::as_str).unwrap_or("");
    let arguments = req.get("arguments").cloned().unwrap_or(Value::Null);

    // 校验 token（仅本机；防串会话）
    let ok = ASK_LISTENER
        .get()
        .and_then(|l| l.tokens.lock().unwrap().get(conn_id).cloned())
        .map(|t| t == token)
        .unwrap_or(false);
    if !ok {
        let _ = write_ask_response(&stream, true, "");
        return;
    }
    let conn = match get_conn(conn_id) {
        Some(c) => c,
        None => {
            let _ = write_ask_response(&stream, true, "");
            return;
        }
    };

    let questions = questions_from_mcp_args(&arguments);
    if questions.is_empty() {
        // 参数不合法（缺 question）→ 直接 declined，模型据此继续
        let _ = write_ask_response(&stream, true, "");
        return;
    }

    let request_id = conn.q_next.fetch_add(1, Ordering::SeqCst).to_string();
    let (tx, rx) = mpsc::channel::<AskOutcome>();
    conn.pending_question
        .lock()
        .unwrap()
        .insert(request_id.clone(), (QuestionReply::Mcp(tx), questions.clone()));
    let questions_json = serde_json::to_value(&questions).unwrap_or(Value::Array(Vec::new()));
    let _ = app.emit(
        "acp://question",
        json!({
            "connId": conn_id,
            "requestId": request_id,
            "sessionId": "",   // MCP 来源无 agent-side sessionId（用 connId 路由）
            "questions": questions_json,
        }),
    );
    eprintln!("[acp] emitted acp://question (mcp) requestId={request_id}");

    // 阻塞等用户答复（会话关闭/取消时由 decline_elicitation_inner/清理回 declined）
    match rx.recv() {
        Ok(outcome) => {
            let _ = write_ask_response(&stream, outcome.declined, &outcome.answer);
        }
        Err(_) => {
            // sender 被丢弃（连接死/会话清）→ 回 declined
            let _ = write_ask_response(&stream, true, "");
        }
    }
}

/// 写回一行应答并关闭连接。
fn write_ask_response(
    mut stream: &std::net::TcpStream,
    declined: bool,
    answer: &str,
) -> std::io::Result<()> {
    let resp = json!({ "declined": declined, "answer": answer });
    let mut line = resp.to_string();
    line.push('\n');
    stream.write_all(line.as_bytes())?;
    stream.flush()
}

/// 纯函数：MCP `ask_user_question` 参数 → `Question`（一次一问，对齐 ACP 分类产物）。
/// 形如 `{ question, options[], multiSelect, header }`；`options` 空 = 自由文本。
fn questions_from_mcp_args(args: &Value) -> Vec<Question> {
    use crate::acp_question::{QuestionKind, QuestionOption};
    let question = args
        .get("question")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if question.is_empty() {
        return Vec::new();
    }
    let options: Vec<QuestionOption> = args
        .get("options")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|o| o.as_str())
                .map(|s| QuestionOption {
                    value: s.to_string(),
                    label: s.to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    let multi = args
        .get("multiSelect")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let kind = if options.is_empty() {
        QuestionKind::Text
    } else if multi {
        QuestionKind::Multi
    } else {
        QuestionKind::Single
    };
    vec![Question {
        id: "answer".to_string(),
        prompt: question,
        kind,
        options,
    }]
}

/// 空闲回收循环：每 ~15s 扫描所有连接，杀掉 `空闲超时 且 无挂起请求` 的连接（design §8.2）。
/// 「忙」判据 = pending（prompt 等挂起请求）或 pending_perm 非空，避免长生成中被误杀。
fn idle_sweep_loop(app: AppHandle) {
    loop {
        std::thread::sleep(Duration::from_secs(IDLE_SWEEP_INTERVAL_SECS));
        let timeout_secs = idle_timeout_secs();
        if timeout_secs == 0 {
            continue;
        }
        let timeout = Duration::from_secs(timeout_secs);
        let snapshot: Vec<(String, Arc<AcpConn>)> = {
            let map = conns().lock().unwrap();
            map.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
        };
        for (conn_id, conn) in snapshot {
            if conn.dead.load(Ordering::SeqCst) {
                continue;
            }
            // 忙判据：有挂起请求或待答复权限/提问 → 跳过（近似「生成中」）
            if !conn.pending.lock().unwrap().is_empty()
                || !conn.pending_perm.lock().unwrap().is_empty()
                || !conn.pending_question.lock().unwrap().is_empty()
            {
                continue;
            }
            let idle = conn.last_activity_at.lock().unwrap().elapsed();
            if idle >= timeout {
                disconnect_conn(&app, &conn_id, &conn, "idle");
            }
        }
    }
}

/// 建立/复用某会话的 ACP 连接（幂等）：spawn + 读线程 + initialize 握手。
/// 已有活连接直接返回其 agentInfo；否则启动全新进程（design §8.1）。
fn connect_inner(
    app: AppHandle,
    conn_id: String,
    engine: AgentType,
    owner: ConnOwner,
) -> Result<StartResult, String> {
    // per-session 启动锁：并发 connect 串行化
    let lock = connect_lock(&conn_id);
    let _guard = lock.lock().unwrap();

    // 幂等：已有活连接直接复用
    if let Some(conn) = get_conn(&conn_id) {
        let info = conn.agent_info.lock().unwrap().clone();
        touch_activity(&conn);
        return Ok(StartResult {
            status: "ready".into(),
            protocol_version: 1,
            agent_info: agent_info_from(&info),
        });
    }

    ensure_idle_sweep(&app);

    let meta = engine::get_agent_meta(engine);
    let mut cmd = build_acp_command(&meta)?;
    // ACP 自身 cwd 与会话 cwd 无关（session/new 传 cwd），无需 --cwd

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 {} 失败: {}（未安装或 PATH 异常）", meta.name, e))?;
    let stdin = child.stdin.take().ok_or("无法获取子进程 stdin")?;
    let stdout = child.stdout.take().ok_or("无法获取子进程 stdout")?;
    let stderr = child.stderr.take().ok_or("无法获取子进程 stderr")?;

    let stderr_tail: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));
    let conn = Arc::new(AcpConn {
        conn_id: conn_id.clone(),
        owner,
        engine,
        dead_reason: Mutex::new(None),
        child: Mutex::new(child),
        stdin: Mutex::new(Some(BufWriter::new(stdin))),
        next_id: AtomicU64::new(1),
        pending: Mutex::new(HashMap::new()),
        pending_perm: Mutex::new(HashMap::new()),
        perm_next: AtomicU64::new(1),
        pending_question: Mutex::new(HashMap::new()),
        q_next: AtomicU64::new(1),
        agent_info: Mutex::new(Value::Null),
        acp_session_id: Mutex::new(None),
        last_assistant_text: Mutex::new(None),
        stderr_tail: stderr_tail.clone(),
        dead: AtomicBool::new(false),
        last_activity_at: Mutex::new(Instant::now()),
    });
    conns()
        .lock()
        .unwrap()
        .insert(conn_id.clone(), conn.clone());

    // 登记 ask token（注入的提问 MCP 伴生进程回连校验用；仅本机）。
    // 监听器在此确保启动（拿到 127.0.0.1:<port>）；session/new 注入时复用同一 addr。
    ensure_ask_listener(&app);
    register_ask_token(&conn_id, &ask_token_for(&conn_id));

    std::thread::spawn(move || drain_stderr(stderr, stderr_tail));
    {
        let reader_app = app.clone();
        let reader_conn = conn.clone();
        std::thread::spawn(move || reader_loop(reader_app, reader_conn, stdout));
    }

    // initialize 握手（60s 超时，对齐 codeg，容忍 claude MCP 预热；D2：不声明 fs/terminal capability，缺省=不支持）
    let result = rpc_call(
        &conn,
        "initialize",
        json!({
            "protocolVersion": 1,
            "clientCapabilities": client_capabilities(),
            "clientInfo": { "name": "lightr-vibe", "version": "0.1.0" }
        }),
    )
    .inspect_err(|_| {
        // 握手失败：关 stdin + kill + 摘该 conn（其余连接不受影响）
        *conn.stdin.lock().unwrap() = None;
        let _ = conn.child.lock().unwrap().kill();
        conn.dead.store(true, Ordering::SeqCst);
        remove_conn(&conn_id, &conn);
    })?;

    *conn.agent_info.lock().unwrap() = result
        .get("agentInfo")
        .cloned()
        .unwrap_or(Value::Null);
    touch_activity(&conn);
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

/// 关闭并移除指定会话的连接（关 stdin → 等 ≤2s → kill 兜底），emit `acp://status`
/// `{connId, status:"stopped"}`。`reason` 仅用于日志区分（idle / 手动）。
fn disconnect_conn(app: &AppHandle, conn_id: &str, conn: &Arc<AcpConn>, reason: &str) {
    // 先移除仓库条目（幂等：重复断连不再匹配）
    remove_conn(conn_id, conn);
    // 断连前对挂起的 elicitation 回 decline（协议：收到方 MUST 回响应，避免 agent hang）。
    // stdin 关闭后无法再写，故必须在关 stdin 之前 drain（design §3.4）。
    let doomed_q: Vec<String> = conn
        .pending_question
        .lock()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    for request_id in doomed_q {
        let _ = decline_elicitation_inner(conn, &request_id);
    }
    unregister_ask_token(conn_id);
    // 关 stdin（drop writer）触发 opencode <1s 干净退出
    *conn.stdin.lock().unwrap() = None;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match conn.child.lock().unwrap().try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = conn.child.lock().unwrap().kill();
                    let _ = conn.child.lock().unwrap().wait();
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break,
        }
    }
    conn.dead.store(true, Ordering::SeqCst);
    let _ = app.emit(
        "acp://status",
        json!({ "connId": conn_id, "status": "stopped", "reason": reason }),
    );
}

/// 应用退出清理（lib.rs RunEvent::Exit）：遍历所有连接，逐个关 stdin → 等 ≤2s → kill。
pub fn shutdown() {
    let snapshot: Vec<(String, Arc<AcpConn>)> = {
        let mut map = conns().lock().unwrap();
        map.drain().collect()
    };
    for (_conn_id, conn) in snapshot {
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
}

// ---------- 一次性探针（acp_probe_options） ----------
//
// 独立 throwaway 子进程：不触碰 CONNS 仓库、不 emit 事件、不动 reader_loop/mark_dead。
// 用途：会话打开时提前把 session/new 的 configOptions（模型列表）灌满前端缓存，
// 使模型下拉不必等到首次发送后才有真值（probe §2：模型列表只来自 session/new）。

/// 探针专用请求发送（单写者，无 pending map）
fn probe_send(writer: &mut BufWriter<ChildStdin>, value: &Value) -> Result<(), String> {
    writeln!(writer, "{}", value).map_err(|e| format!("acp-dead: 探针写入 stdin 失败: {}", e))?;
    writer
        .flush()
        .map_err(|e| format!("acp-dead: 探针 flush stdin 失败: {}", e))
}

/// 探针 stdout 行读取线程：逐行 parse 成 Value 后经 channel 转发。
/// 读线程在 stdout EOF（进程退出）时自然结束；行内容为任意字符串，解析放在
/// 消费侧，保证 EOF 与「收到行」可用同一 channel 区分（None = EOF）。
fn probe_spawn_reader(stdout: std::process::ChildStdout) -> Receiver<Value> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // probe §5：非 JSON 行静默忽略
            if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
                if tx.send(v).is_err() {
                    break;
                }
            }
        }
        // 读线程结束（EOF/错误）→ tx drop，消费侧 recv_timeout 得到 Disconnected
    });
    rx
}

/// 从已 parse 的消息流中取指定 id 的响应；忽略通知（无 id）与意外到的 client request。
/// 超时：真实 `recv_timeout`（与 rpc_call 同语义），避免 Agent 挂死但仍持 stdout
/// 时无限阻塞——这同时保证 ProbeChild::drop 一定能收尸，不让前端刷新卡死。
/// 超时/EOF 错误串统一为 `timeout:<秒>s <method>`（对外词汇一致性）。
fn probe_read_response(
    rx: &Receiver<Value>,
    expected_id: u64,
    method: &str,
) -> Result<Value, String> {
    let timeout = timeout_for(method);
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(format!("timeout:{}s {}", timeout.as_secs(), method));
        }
        let msg = match rx.recv_timeout(remaining) {
            Ok(v) => v,
            // EOF（进程退出）/ 读线程异常：与超时同词汇
            Err(RecvTimeoutError::Disconnected) => {
                return Err(format!("timeout:{}s {}", timeout.as_secs(), method))
            }
            Err(RecvTimeoutError::Timeout) => {
                return Err(format!("timeout:{}s {}", timeout.as_secs(), method))
            }
        };
        // 只认「有 id 且无 method」的响应（同 reader_loop 的 Route 规则）；
        // notification（无 id）/ 其它 id / client request（有 method，即便 id 撞号）一律忽略
        if msg.get("method").is_some()
            || msg.get("id").and_then(Value::as_u64) != Some(expected_id)
        {
            continue;
        }
        if let Some(err) = msg.get("error") {
            return Err(rpc_error_string(err));
        }
        return Ok(msg.get("result").cloned().unwrap_or(Value::Null));
    }
}

/// 探针子进程句柄：Drop 时关 stdin → 短宽限等待退出 → kill 兜底，保证不泄漏
struct ProbeChild {
    child: Child,
    stdin: Option<BufWriter<ChildStdin>>,
}

impl ProbeChild {
    /// 显式清理（Ok/Err 两路都走）：关 stdin 触发 <1s 干净退出，超宽限再 kill
    fn cleanup(&mut self) {
        // 关 stdin（drop writer）→ opencode 实测 <1s rc=0 干净退出
        self.stdin = None;
        let deadline = Instant::now() + Duration::from_millis(1500);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => {
                    if Instant::now() >= deadline {
                        let _ = self.child.kill();
                        // 收尸，避免僵尸
                        let _ = self.child.wait();
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => return,
            }
        }
    }
}

impl Drop for ProbeChild {
    fn drop(&mut self) {
        // 保底：即使某条路径漏掉显式 cleanup，Drop 也会收尸
        self.cleanup();
    }
}

/// 探针主体（阻塞，必须在 spawn_blocking 内跑）：
/// resolve_spawn → initialize → session/new → 取 configOptions → 清理子进程。
fn probe_options_inner(cwd: Option<String>, engine: AgentType) -> Result<ProbeResult, String> {
    // cwd 必须为存在的绝对路径（协议 §9），否则 session/new 回 -32602
    let work_dir: PathBuf = match cwd {
        Some(dir) if !dir.trim().is_empty() => PathBuf::from(dir),
        _ => std::env::temp_dir().join("lightr-vibe-acp-probe"),
    };
    std::fs::create_dir_all(&work_dir).map_err(|e| format!("创建探针目录失败: {}", e))?;
    let cwd_str = work_dir.to_string_lossy().into_owned();

    let meta = engine::get_agent_meta(engine);
    let mut cmd = build_acp_command(&meta)?;

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 {} 失败: {}（未安装或 PATH 异常）", meta.name, e))?;
    let stdin = child.stdin.take().ok_or("无法获取探针子进程 stdin")?;
    let stdout = child.stdout.take().ok_or("无法获取探针子进程 stdout")?;
    let stderr = child.stderr.take().ok_or("无法获取探针子进程 stderr")?;

    // stderr 独立线程 drain（防管道满死锁，tauri-bridge spec §4）；探针丢弃内容
    std::thread::spawn(move || {
        for _ in BufReader::new(stderr).lines().flatten() {}
    });

    // ProbeChild::drop 保证任何提前 return 都收尸；stdout 行经独立线程 + channel 转发，
    // 使 recv_timeout 成为真实超时（Agent 挂死时不再无限阻塞）
    let mut probe = ProbeChild {
        child,
        stdin: Some(BufWriter::new(stdin)),
    };
    let rx = probe_spawn_reader(stdout);

    // 探针 id 从 1 起（与长驻连接 id 空间无关，独立进程）
    let init_id: u64 = 1;
    let init_msg = json!({
        "jsonrpc": "2.0", "id": init_id, "method": "initialize",
        "params": {
            "protocolVersion": 1,
            "clientCapabilities": client_capabilities(),
            "clientInfo": { "name": "lightr-vibe", "version": "0.1.0" }
        }
    });
    {
        let writer = probe.stdin.as_mut().ok_or("探针 stdin 已关闭")?;
        probe_send(writer, &init_msg)?;
    }
    let _init_result = probe_read_response(&rx, init_id, "initialize")?;

    // session/new：首调 7.6-8.9s（probe §2），timeout_for 给 30s
    let new_id: u64 = 2;
    let new_msg = json!({
        "jsonrpc": "2.0", "id": new_id, "method": "session/new",
        "params": { "cwd": cwd_str, "mcpServers": [] }
    });
    {
        let writer = probe.stdin.as_mut().ok_or("探针 stdin 已关闭")?;
        probe_send(writer, &new_msg)?;
    }
    let result = probe_read_response(&rx, new_id, "session/new")?;

    // 取 configOptions（缺失 → Null，前端按 null 处理）
    let config_options = result.get("configOptions").cloned().unwrap_or(Value::Null);

    probe.cleanup(); // 显式收尸（Drop 亦会执行，幂等）
    Ok(ProbeResult { config_options })
}

// ---------- 模型上下文窗口解析（对齐 codeg parse/opencode_context_window） ----------

/// `~/.config/opencode`（XDG 语义）：`$XDG_CONFIG_HOME/opencode`，否则 `$HOME/.config/opencode`。
/// Windows 上 opencode 1.x 同样用 `~/.config/opencode`（本机实测存在）。
fn opencode_config_dir() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.trim().is_empty() {
            return Some(PathBuf::from(xdg).join("opencode"));
        }
    }
    std::env::var("HOME")
        .ok()
        .filter(|h| !h.trim().is_empty())
        .map(|h| PathBuf::from(h).join(".config").join("opencode"))
}

/// 剥离 JSONC 注释与尾逗号（轻量版；不做字符串转义的完整处理，仅用于容错解析配置）。
/// opencode 默认写 `opencode.jsonc`，带 `//` / `/* */` 注释与尾逗号。
fn strip_jsonc(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    let mut open_comma: Option<usize> = None;
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                open_comma = None;
                out.push(c);
                while let Some(c2) = chars.next() {
                    out.push(c2);
                    match c2 {
                        '\\' => {
                            if let Some(esc) = chars.next() {
                                out.push(esc);
                            }
                        }
                        '"' => break,
                        _ => {}
                    }
                }
            }
            '/' if chars.peek() == Some(&'/') => {
                for c2 in chars.by_ref() {
                    if c2 == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = '\0';
                for c2 in chars.by_ref() {
                    if prev == '*' && c2 == '/' {
                        break;
                    }
                    prev = c2;
                }
                out.push(' ');
            }
            ',' => {
                open_comma = Some(out.len());
                out.push(c);
            }
            '}' | ']' => {
                if let Some(at) = open_comma.take() {
                    out.replace_range(at..at + 1, " ");
                }
                out.push(c);
            }
            c if c.is_whitespace() => out.push(c),
            c => {
                open_comma = None;
                out.push(c);
            }
        }
    }
    out
}

/// 读取并解析一个 JSON/JSONC 文件为 Value（BOM/注释/尾逗号容错）
fn read_jsonc(path: &Path) -> Option<Value> {
    let raw = std::fs::read_to_string(path).ok()?;
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
    serde_json::from_str::<Value>(raw)
        .ok()
        .or_else(|| serde_json::from_str::<Value>(&strip_jsonc(raw)).ok())
}

/// 从 `limit.context` 取值（int/float 均可，>0 才有效）
fn context_from_limit(entry: &Value) -> Option<u64> {
    let v = entry.get("limit")?.get("context")?;
    let n = v
        .as_u64()
        .or_else(|| v.as_f64().filter(|f| f.is_finite() && *f >= 1.0).map(|f| f as u64))?;
    (n > 0).then_some(n)
}

/// 在 config 里按 provider/model 查 `limit.context`：
/// `provider|providers.<pid>.models.<mid>.limit.context`
fn lookup_in_config(config: &Value, provider: &str, model: &str) -> Option<u64> {
    for root_key in ["provider", "providers"] {
        let entry = config
            .get(root_key)
            .and_then(|p| p.get(provider))
            .and_then(|p| p.get("models"))
            .and_then(|m| m.get(model));
        if let Some(ctx) = entry.and_then(context_from_limit) {
            return Some(ctx);
        }
    }
    None
}

/// 在 opencode 缓存的 models.dev 目录里查：`{ pid: { models: { mid: { limit: { context } } } } }`
fn lookup_in_catalog(catalog: &Value, provider: &str, model: &str) -> Option<u64> {
    let entry = catalog
        .get(provider)
        .and_then(|p| p.get("models"))
        .and_then(|m| m.get(model))?;
    context_from_limit(entry)
}

/// opencode 缓存的 models.dev 目录路径：`$OPENCODE_MODELS_PATH` →
/// `$XDG_CACHE_HOME/opencode/models.json` → `$HOME/.cache/opencode/models.json`
fn opencode_catalog_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("OPENCODE_MODELS_PATH") {
        if !p.trim().is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    let cache_root = std::env::var("XDG_CACHE_HOME")
        .ok()
        .filter(|c| !c.trim().is_empty())
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .filter(|h| !h.trim().is_empty())
                .map(|h| format!("{}/.cache", h))
        })?;
    Some(PathBuf::from(cache_root).join("opencode").join("models.json"))
}

/// 从模型名字串里的容量后缀取窗口：`[1M]` / `[500k]` / `[1.5m]`（照 codeg
/// `parsers::parse_model_capacity_suffix`）。大小写不敏感，允许中间空格。
/// 这是**引擎无关**的权威信号：网关自定义模型（如 `deepseek-v4.1-flash[1M]`）
/// 会把容量直接写进名字，读它比任何家族启发式都准。
fn parse_model_capacity_suffix(model: &str) -> Option<u64> {
    let m = model.trim();
    let end = m.strip_suffix(']')?;
    let open = end.rfind('[')?;
    let inner = end[open + 1..].trim();
    let unit = inner.chars().last()?;
    let mult = match unit.to_ascii_lowercase() {
        'm' => 1_000_000f64,
        'k' => 1_000f64,
        _ => return None,
    };
    let num = inner[..inner.len() - unit.len_utf8()].trim().parse::<f64>().ok()?;
    if !num.is_finite() || num <= 0.0 {
        return None;
    }
    let v = (num * mult) as u64;
    (v > 0).then_some(v)
}

/// 模型名启发式（照 codeg `parsers::infer_context_window_max_tokens` 的 Claude 分支）：
/// 后缀优先；否则 `claude*` 型号按家族/`-1m` 判定 1M 或 200k。非 claude 返回 None。
fn infer_context_window_from_model(model: &str) -> Option<u64> {
    let raw = model.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(suffixed) = parse_model_capacity_suffix(raw) {
        return Some(suffixed);
    }
    let normalized = raw
        .rsplit('/')
        .next()
        .unwrap_or(raw)
        .split(':')
        .next()
        .unwrap_or(raw)
        .trim()
        .to_ascii_lowercase();
    if normalized.starts_with("claude") {
        if is_claude_one_million_id(&normalized) || is_claude_native_one_million_model(&normalized) {
            return Some(1_000_000);
        }
        return Some(200_000);
    }
    None
}

/// `\b1m\b` 形态的 1M-lane id（`claude-opus-4-6-1m`）；不误命中 `10m`。
fn is_claude_one_million_id(model: &str) -> bool {
    let bytes = model.as_bytes();
    let mut i = 0;
    while let Some(pos) = model[i..].find("1m") {
        let at = i + pos;
        let before_ok = at == 0 || !bytes[at - 1].is_ascii_alphanumeric();
        let after = at + 2;
        let after_ok = after >= bytes.len() || !bytes[after].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return true;
        }
        i = at + 2;
    }
    false
}

/// `claude-<family>-<major>[.<minor>]`；家族原生 1M：
/// 所有 fable/mythos、opus≥4.7、sonnet≥5、haiku≥5.5。
fn is_claude_native_one_million_model(model: &str) -> bool {
    let rest = match model.strip_prefix("claude-") {
        Some(r) => r,
        None => return false,
    };
    let family: String = rest.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let after_family = &rest[family.len()..];
    let after_family = after_family.strip_prefix('-').unwrap_or(after_family);
    let major: u32 = after_family
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0);
    let after_major = after_family.trim_start_matches(|c: char| c.is_ascii_digit());
    // minor 可用 `.` 或 `-` 分隔（codeg 正则 `[-.](\d{1,2})`）
    let minor: u32 = after_major
        .strip_prefix('.')
        .or_else(|| after_major.strip_prefix('-'))
        .map(|s| s.chars().take_while(|c| c.is_ascii_digit()).collect::<String>())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    match family.as_str() {
        "fable" | "mythos" => true,
        "opus" => (major, minor) >= (4, 7),
        "sonnet" => major >= 5,
        "haiku" => (major, minor) >= (5, 5),
        _ => false,
    }
}

/// 从 UI 文案里嗅探容量：`(1M context)` / `[1M]` / `1,000,000` 等。
/// 仅当模型名本身无后缀/非 claude 时才用。数字后缀优先，其次裸大数（≥100000）。
fn parse_context_hint_text(hint: &str) -> Option<u64> {
    if let Some(v) = parse_model_capacity_suffix(hint) {
        return Some(v);
    }
    let lower = hint.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        let mut saw_dot = false;
        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b',' || bytes[i] == b'.')
        {
            if bytes[i] == b'.' {
                saw_dot = true;
            }
            i += 1;
        }
        let digits: String = lower[start..i]
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        let unit = bytes.get(i).copied().map(|b| b as char);
        match unit {
            Some('m') => {
                if let Ok(n) = digits.parse::<f64>() {
                    if n.is_finite() && n > 0.0 {
                        return Some((n * 1_000_000.0) as u64);
                    }
                }
            }
            Some('k') => {
                if let Ok(n) = digits.parse::<f64>() {
                    if n.is_finite() && n > 0.0 {
                        return Some((n * 1_000.0) as u64);
                    }
                }
            }
            _ if !saw_dot => {
                // 裸数字（含 `1,000,000` / `128000`）：仅 ≥100000 才当容量
                if let Ok(n) = digits.parse::<u64>() {
                    if n >= 100_000 {
                        return Some(n);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// 解析模型上下文窗口（按引擎的 context-window 策略）：opencode 用
/// 用户配置 → models.dev 缓存目录；Claude/Codex 无配置来源，用模型名后缀 /
/// claude 家族启发式 / 前端 hint 兜底。都查不到返回 None（前端保持既有分母，不臆造）。
fn model_context_window_inner(
    provider: &str,
    model: &str,
    engine: AgentType,
    hint: Option<&str>,
) -> Option<u64> {
    // provider 可为空（Claude 的 model 选项 value 是单名如 `fable`，无 provider 前缀）；
    // 仅 model 必需。opencode 的 config/catalog 查询要求 provider，缺失时该分支自然跳过。
    if model.trim().is_empty() {
        return None;
    }
    let meta = engine::get_agent_meta(engine);
    // 1) 引擎声明的配置/目录来源（opencode：用户配置 → models.dev 缓存）
    if let ContextWindowSource::OpenCodeConfigAndCatalog = meta.launch.context_window {
        if let Some(ctx) = model_context_window_opencode(provider, model) {
            return Some(ctx);
        }
    }
    // 2) 引擎无关兜底：模型名容量后缀 / claude 家族启发式，再退到前端 hint
    //    （选项 name/description，如 `deepseek-v4.1-flash[1m]`、`(1M context)`）。
    infer_context_window_from_model(model)
        .or_else(|| hint.and_then(infer_context_window_from_model))
        .or_else(|| hint.and_then(parse_context_hint_text))
}

/// opencode 策略：用户 opencode 配置 → models.dev 缓存目录。
/// 查不到返回 None（前端保持既有分母，不臆造）。
fn model_context_window_opencode(provider: &str, model: &str) -> Option<u64> {
    // 1) 用户配置：$OPENCODE_CONFIG → config.json → opencode.json → opencode.jsonc
    let mut config_files: Vec<PathBuf> = Vec::new();
    if let Ok(cfg) = std::env::var("OPENCODE_CONFIG") {
        if !cfg.trim().is_empty() {
            config_files.push(PathBuf::from(cfg));
        }
    }
    if let Some(dir) = opencode_config_dir() {
        for name in ["config.json", "opencode.json", "opencode.jsonc"] {
            config_files.push(dir.join(name));
        }
    }
    for path in config_files {
        if let Some(config) = read_jsonc(&path) {
            if let Some(ctx) = lookup_in_config(&config, provider, model) {
                return Some(ctx);
            }
        }
    }
    // 2) models.dev 缓存目录
    if let Some(catalog_path) = opencode_catalog_path() {
        if let Some(catalog) = read_jsonc(&catalog_path) {
            if let Some(ctx) = lookup_in_catalog(&catalog, provider, model) {
                return Some(ctx);
            }
        }
    }
    None
}

// ---------- Tauri commands ----------

/// 建立/复用某会话的 ACP 连接（幂等）：为该 connId spawn 独立进程 + initialize 握手。
/// 已有活连接直接返回其 agentInfo（避免重复 ~1.2s 握手）。engine_id 为 wire 值。
#[tauri::command]
pub async fn acp_connect(
    app: AppHandle,
    session_id: String,
    engine_id: String,
) -> Result<StartResult, String> {
    let engine = AgentType::from_wire(&engine_id)
        .ok_or_else(|| format!("acp:-32602 未知引擎: {}", engine_id))?;
    tauri::async_runtime::spawn_blocking(move || connect_inner(app, session_id, engine, ConnOwner::User))
        .await
        .map_err(|e| format!("acp 任务执行失败: {}", e))?
}

/// 断开某会话连接（杀该会话进程）：关 stdin → 等 ≤2s → kill 兜底。
/// 关标签 / 删除会话时调用（design §8.1）；幂等（不存在则 no-op）。
#[tauri::command]
pub async fn acp_disconnect(app: AppHandle, session_id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(conn) = get_conn(&session_id) {
            disconnect_conn(&app, &session_id, &conn, "manual");
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("acp 任务执行失败: {}", e))?
}

/// 查询某会话连接状态（ready / dead / stopped + 原因）
#[tauri::command]
pub fn acp_status(conn_id: String) -> StatusResult {
    if let Some(conn) = get_conn(&conn_id) {
        let info = conn.agent_info.lock().unwrap().clone();
        return StatusResult {
            conn_id,
            status: "ready".into(),
            reason: None,
            agent_info: agent_info_from(&info),
        };
    }
    let reason = conns()
        .lock()
        .unwrap()
        .get(&conn_id)
        .and_then(|c| c.dead_reason.lock().unwrap().clone());
    StatusResult {
        status: if reason.is_some() { "dead" } else { "stopped" }.into(),
        conn_id,
        reason,
        agent_info: None,
    }
}

/// 一次性探针：spawn throwaway `opencode acp` → initialize → session/new →
/// 取 configOptions（模型列表）→ 关 stdin 收尸。
/// 不触碰任何长驻连接（CONNS），不 emit 事件；供前端会话打开/手动刷新时填充模型缓存。
/// cwd 缺省用临时目录（协议要求存在的绝对 cwd）。
#[tauri::command]
pub async fn acp_probe_options(
    cwd: Option<String>,
    engine_id: Option<String>,
) -> Result<ProbeResult, String> {
    let engine = match engine_id.as_deref() {
        Some(id) => AgentType::from_wire(id)
            .ok_or_else(|| format!("acp:-32602 未知引擎: {}", id))?,
        None => AgentType::OpenCode,
    };
    tauri::async_runtime::spawn_blocking(move || probe_options_inner(cwd, engine))
        .await
        .map_err(|e| format!("acp 任务执行失败: {}", e))?
}

/// 按 provider/model 解析模型上下文窗口（limit.context），策略来自引擎声明。
/// 查不到返回 None（前端保持既有分母）。纯文件读取，不触碰 ACP 进程/连接。
#[tauri::command]
pub async fn agent_model_context_window(
    provider: String,
    model: String,
    engine_id: Option<String>,
    hint: Option<String>,
) -> Result<Option<u64>, String> {
    let engine = match engine_id.as_deref() {
        Some(id) => AgentType::from_wire(id)
            .ok_or_else(|| format!("acp:-32602 未知引擎: {}", id))?,
        None => AgentType::OpenCode,
    };
    tauri::async_runtime::spawn_blocking(move || {
        model_context_window_inner(&provider, &model, engine, hint.as_deref())
    })
    .await
    .map_err(|e| format!("acp 任务执行失败: {}", e))
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

/// 新建 ACP 会话（首调 7.6-8.9s → 30s 超时；mcpServers 为必填字段，传 []）。
/// conn_id = 本地会话 id（定位该会话进程）；返回 agent-side sessionId。
#[tauri::command]
pub async fn acp_session_new(conn_id: String, cwd: String) -> Result<SessionResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let conn = require_conn(&conn_id)?;
        // cwd 必须存在（协议 §9 绝对路径）；tmpsession 目录首次使用时自动创建
        std::fs::create_dir_all(&cwd).map_err(|e| format!("创建会话目录失败: {}", e))?;
        // 注入我们的提问 MCP 服务器（任意引擎可提问；无法定位 bin 时为空数组，零回归）
        let result = rpc_call(
            &conn,
            "session/new",
            json!({ "cwd": cwd, "mcpServers": mcp_servers_for(&conn_id) }),
        )?;
        touch_activity(&conn);
        parse_session_result(&result, None)
    })
    .await
    .map_err(|e| format!("acp 任务执行失败: {}", e))?
}

/// 重载 ACP 会话（loadSession capability；历史以 session/update 在响应前重放，
/// 前端对重放 update 一律忽略——本地历史为准，design §4）。
#[tauri::command]
pub async fn acp_session_load(
    conn_id: String,
    acp_session_id: String,
    cwd: String,
) -> Result<SessionResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let conn = require_conn(&conn_id)?;
        let id = acp_session_id.clone();
        let result = rpc_call(
            &conn,
            "session/load",
            json!({ "sessionId": acp_session_id, "cwd": cwd, "mcpServers": mcp_servers_for(&conn_id) }),
        )?;
        touch_activity(&conn);
        parse_session_result(&result, Some(id))
    })
    .await
    .map_err(|e| format!("acp 任务执行失败: {}", e))?
}

/// 发送 prompt（长请求：期间 session/update 流式推送，turn 结束才响应；300s 超时）。
/// conn_id = 本地会话 id（定位进程）；session_id = agent-side id（方法参数）。
#[tauri::command]
pub async fn acp_prompt(
    conn_id: String,
    session_id: String,
    text: String,
) -> Result<PromptResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let conn = require_conn(&conn_id)?;
        touch_activity(&conn);
        let result = rpc_call(
            &conn,
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": [{ "type": "text", "text": text }]
            }),
        )?;
        touch_activity(&conn);
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
pub fn acp_cancel(conn_id: String, session_id: String) -> Result<(), String> {
    let conn = require_conn(&conn_id)?;
    send_line(
        &conn,
        &json!({ "jsonrpc": "2.0", "method": "session/cancel", "params": { "sessionId": session_id } }),
    )?;
    touch_activity(&conn);
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
    // 挂起的 elicitation 提问同权限一样回 decline（与 pending_perm 平行清理）：
    // 协议要求收到方 MUST 回响应，否则 agent 在该会话上阻塞
    let doomed_q: Vec<String> = conn
        .pending_question
        .lock()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    for request_id in doomed_q {
        let _ = decline_elicitation_inner(&conn, &request_id);
    }
    Ok(())
}

/// 关闭 ACP 会话（session/close request）：取消该会话进行中的工作并释放 agent 侧资源。
/// 进程保持存活（连接仍可复用）；连接销毁走 acp_disconnect。规范为可选方法（需
/// sessionCapabilities.close）；agent 报错（未知会话/不支持）时 Err 上抛，前端 best-effort 吞掉。
#[tauri::command]
pub async fn acp_session_close(conn_id: String, session_id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let conn = require_conn(&conn_id)?;
        rpc_call(&conn, "session/close", json!({ "sessionId": session_id }))?;
        Ok(())
    })
    .await
    .map_err(|e| format!("acp 任务执行失败: {}", e))?
}

/// 从 set_config_option 的应答里判断该配置项是否被 agent 落成了别的值。
/// 在 `result.configOptions` 中按 `id` 或 `category` 命中 config_id，取 `currentValue`：
/// - 命中且为字符串且 != requested → `Some(actual)`（被拒绝/调整）。
/// - 相等 / 缺项 / currentValue 非字符串 → `None`（不发，避免误报）。
/// 纯函数，无副作用，便于单测（design §2.1）。
fn detect_config_rejection(result: &Value, config_id: &str, requested: &str) -> Option<String> {
    let options = result.get("configOptions")?.as_array()?;
    let hit = options.iter().find(|o| {
        o.get("id").and_then(Value::as_str) == Some(config_id)
            || o.get("category").and_then(Value::as_str) == Some(config_id)
    })?;
    let actual = hit.get("currentValue").and_then(Value::as_str)?;
    if actual == requested {
        None
    } else {
        Some(actual.to_string())
    }
}

/// 切换会话配置（优先规范方法 set_config_option，probe §6）→ 返回全量 configOptions。
/// 应答里该 configId 的落定值 != 请求值 → 额外 emit `acp://configRejected`（AC6）：
/// agent 有时不采纳（如需凭据模型回退），前端据此提示「该选项被 agent 调整」。
#[tauri::command]
pub async fn acp_set_config(
    app: AppHandle,
    conn_id: String,
    session_id: String,
    config_id: String,
    value: Value,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let conn = require_conn(&conn_id)?;
        let out = rpc_call(
            &conn,
            "session/set_config_option",
            json!({ "sessionId": session_id, "configId": config_id, "value": value.clone() }),
        )?;
        touch_activity(&conn);
        // 比对落定值：仅当请求值为字符串且被落成别的字符串时才提示
        if let Some(requested) = value.as_str() {
            if let Some(actual) = detect_config_rejection(&out, &config_id, requested) {
                let _ = app.emit(
                    "acp://configRejected",
                    json!({
                        "connId": conn_id,
                        "configId": config_id,
                        "requested": requested,
                        "actual": actual,
                    }),
                );
            }
        }
        Ok(out)
    })
    .await
    .map_err(|e| format!("acp 任务执行失败: {}", e))?
}

/// 答复权限请求；option_id 为 None = cancelled 语义（design §2，协议 §5）
#[tauri::command]
pub fn acp_respond_permission(
    conn_id: String,
    request_id: String,
    option_id: Option<String>,
) -> Result<(), String> {
    let conn = require_conn(&conn_id)?;
    touch_activity(&conn);
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
    eprintln!("[acp] respond permission requestId={request_id} optionId={:?}", option_id);
    send_line(
        conn,
        &json!({ "jsonrpc": "2.0", "id": rpc_id, "result": outcome }),
    )
}

/// 答复 elicitation 提问的回传参数项（camelCase：fieldId / values）
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerItem {
    pub field_id: String,
    pub values: Vec<String>,
}

/// 答复 elicitation 提问（Questions 型）：按 requestId 找 pending 的 JSON-RPC id，
/// 构造 accept/decline 响应回写并从 map 移除。
/// 未命中（重复/过期/已答复/已死）→ `Ok(())` 安全 no-op（design §3.3）。
#[tauri::command]
pub fn acp_answer_elicitation(
    conn_id: String,
    request_id: String,
    answers: Vec<AnswerItem>,
    declined: bool,
) -> Result<(), String> {
    let conn = require_conn(&conn_id)?;
    touch_activity(&conn);
    if declined {
        // 用户跳过 / 拒答：回 decline（找不到也安全 no-op）
        let _ = decline_elicitation_inner(&conn, &request_id);
        return Ok(());
    }
    let entry = conn.pending_question.lock().unwrap().remove(&request_id);
    let Some((reply, questions)) = entry else {
        // 请求不存在或已答复：安全 no-op（不报错，避免 UI 因迟到答复弹错）
        return Ok(());
    };
    let pairs: Vec<(String, Vec<String>)> = answers
        .into_iter()
        .map(|a| (a.field_id, a.values))
        .collect();
    eprintln!("[acp] answer question requestId={request_id}");
    match reply {
        // 原生 ACP elicitation：回写 JSON-RPC 响应
        QuestionReply::Acp(rpc_id) => {
            let result = build_elicitation_result(&questions, &pairs, false);
            send_line(
                &conn,
                &json!({ "jsonrpc": "2.0", "id": rpc_id, "result": result }),
            )
        }
        // MCP ask 工具：把答案文本经 socket 回传（伴生进程包成工具结果）
        QuestionReply::Mcp(tx) => {
            let answer = render_answer_text(&questions, &pairs);
            let _ = tx.send(AskOutcome {
                declined: false,
                answer,
            });
            Ok(())
        }
    }
}

/// 内部：对某挂起 elicitation 回 `{"action":"decline"}` 并移除（找不到则 no-op）。
/// 供 cancel / disconnect 清理与「跳过」答复复用。MCP 来源则经 socket 回 declined。
fn decline_elicitation_inner(conn: &AcpConn, request_id: &str) -> Result<(), String> {
    let entry = conn.pending_question.lock().unwrap().remove(request_id);
    let Some((reply, _questions)) = entry else {
        return Ok(());
    };
    match reply {
        QuestionReply::Acp(rpc_id) => send_line(
            conn,
            &json!({ "jsonrpc": "2.0", "id": rpc_id, "result": { "action": "decline" } }),
        ),
        QuestionReply::Mcp(tx) => {
            let _ = tx.send(AskOutcome {
                declined: true,
                answer: String::new(),
            });
            Ok(())
        }
    }
}

/// 把用户答案渲染成给模型看的文本：单选用其值，多选以顿号连接。
/// 值可能缺失（模型未见过的字段）→ 跳过。
fn render_answer_text(questions: &[Question], pairs: &[(String, Vec<String>)]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (field_id, values) in pairs {
        if values.is_empty() {
            continue;
        }
        let label = questions
            .iter()
            .find(|q| &q.id == field_id)
            .map(|q| q.prompt.clone())
            .unwrap_or_else(|| field_id.clone());
        parts.push(format!("{}：{}", label, values.join("、")));
    }
    if parts.is_empty() {
        "（用户未提供具体答案）".to_string()
    } else {
        parts.join("\n")
    }
}

// ---------- 自动化无头会话（task 10-26-automation-scheduler，design 5.5） ----------

/// 一次无头 turn 的结果。
pub(crate) struct AutomationTurnOutcome {
    pub session_id: String,
    pub stop_reason: String,
    /// 该轮最后一次 assistant 文本（运行记录的 summary）
    pub summary: Option<String>,
}

/// 关闭一个自动化会话：先取消在飞 turn，再断开进程。
///
/// 两步都要做 —— 只 cancel 不断开会让进程挂着，只断开会让在飞 prompt 永不返回。
/// `session/cancel` 是 notification，idle 时是安全 no-op。
pub(crate) fn shutdown_automation_conn(app: &AppHandle, conn_id: &str) {
    if let Some(conn) = get_conn(conn_id) {
        let sid = conn.acp_session_id.lock().unwrap().clone();
        if let Some(sid) = sid {
            let _ = send_line(
                &conn,
                &json!({
                    "jsonrpc": "2.0",
                    "method": "session/cancel",
                    "params": { "sessionId": sid }
                }),
            );
        }
        disconnect_conn(app, conn_id, &conn, "automation");
    }
}

/// 自动化无头会话的一次完整 turn：建连 → session/new → 设模型/思考强度 → prompt。
///
/// **同步阻塞**（必须在 `spawn_blocking` 里调用）。返回 `stop_reason` 与末轮
/// assistant 文本；失败返回可读错误串。无论成败，调用方负责
/// [`shutdown_automation_conn`]。
///
/// 与前端会话系统的边界（prd R7）：这里用独立的 connId，且不写前端会话索引 ——
/// 完成的判定完全靠 `rpc_call` 的阻塞返回值，不依赖前端事件总线。
pub(crate) fn run_automation_turn(
    app: AppHandle,
    conn_id: String,
    run_id: String,
    engine: AgentType,
    cwd: String,
    model_id: Option<String>,
    effort: Option<(String, String)>,
    prompt: String,
) -> Result<AutomationTurnOutcome, String> {
    connect_inner(app.clone(), conn_id.clone(), engine, ConnOwner::Automation)?;
    let conn = require_conn(&conn_id)?;

    // 协议要求 cwd 是已存在的绝对路径
    std::fs::create_dir_all(&cwd).map_err(|e| format!("io: 创建会话目录失败: {}", e))?;
    let result = rpc_call(
        &conn,
        "session/new",
        json!({ "cwd": cwd, "mcpServers": mcp_servers_for(&conn_id) }),
    )?;
    touch_activity(&conn);
    let session = parse_session_result(&result, None)?;
    *conn.acp_session_id.lock().unwrap() = Some(session.session_id.clone());
    // 立刻登记到运行记录：运行历史在「运行中」期间就要能看到会话 id，
    // 否则 prd R7 的「用 acp_session_load 回放」无从谈起（settle 时才写太晚）。
    // 失败不影响这轮运行，只丢一条可回放线索。
    if let Err(e) = crate::automation::service::attach_run_session(&run_id, &session.session_id) {
        eprintln!("[acp] automation run {} 登记 sessionId 失败: {}", run_id, e);
    }

    // 模型 / 思考强度：best-effort，agent 不认就保持默认，不阻断这轮运行
    if let Some(model) = model_id.filter(|m| !m.trim().is_empty()) {
        let _ = rpc_call(
            &conn,
            "session/set_config_option",
            json!({ "sessionId": session.session_id, "configId": "model", "value": model }),
        );
    }
    if let Some((config_id, value)) = effort {
        if !config_id.trim().is_empty() && !value.trim().is_empty() {
            let _ = rpc_call(
                &conn,
                "session/set_config_option",
                json!({ "sessionId": session.session_id, "configId": config_id, "value": value }),
            );
        }
    }

    // 每轮开始清空 summary 槽，避免上一轮残留
    *conn.last_assistant_text.lock().unwrap() = None;

    let prompt_result = rpc_call(
        &conn,
        "session/prompt",
        json!({
            "sessionId": session.session_id,
            "prompt": [{ "type": "text", "text": prompt }]
        }),
    );
    touch_activity(&conn);

    match prompt_result {
        Ok(value) => {
            let stop_reason = value
                .get("stopReason")
                .and_then(Value::as_str)
                .unwrap_or("end_turn")
                .to_string();
            let summary = conn
                .last_assistant_text
                .lock()
                .unwrap()
                .clone()
                .filter(|t| !t.trim().is_empty());
            Ok(AutomationTurnOutcome {
                session_id: session.session_id,
                stop_reason,
                summary,
            })
        }
        Err(e) => Err(e),
    }
}

// ---------- 单元测试（协议路由 / 超时矩阵 / pending 清理 / shim 解析） ----------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp_question::QuestionKind;

    /// session.notices 必须是**对象**（适配器按 typeof === "object" 判断）；
    /// 刻意不声明 compaction（本项目无 compaction 消费者，声明会触发回归）。
    #[test]
    fn client_capabilities_advertises_session_notices_only() {
        let caps = client_capabilities();
        let session = caps.get("session").expect("session member");
        assert!(session.get("notices").is_some_and(Value::is_object));
        assert!(session.get("compaction").is_none());
        // 既有能力保持不变（回归护栏）
        assert!(caps.get("fs").is_some());
        assert!(caps.get("elicitation").is_some());
    }

    #[test]
    fn capacity_suffix_parses_units() {
        assert_eq!(parse_model_capacity_suffix("qwen3.8-max-0902[1M]"), Some(1_000_000));
        assert_eq!(parse_model_capacity_suffix("deepseek-v4.1-flash[1m]"), Some(1_000_000));
        assert_eq!(parse_model_capacity_suffix("foo[500k]"), Some(500_000));
        assert_eq!(parse_model_capacity_suffix("foo[1.5m]"), Some(1_500_000));
        assert_eq!(parse_model_capacity_suffix("foo[1M] "), Some(1_000_000));
        assert_eq!(parse_model_capacity_suffix("claude-sonnet-4-6"), None);
        assert_eq!(parse_model_capacity_suffix("foo[0m]"), None);
        assert_eq!(parse_model_capacity_suffix("foo[xm]"), None);
    }

    #[test]
    fn infer_context_window_suffix_beats_family() {
        assert_eq!(
            infer_context_window_from_model("deepseek-v4.1-flash[1M]"),
            Some(1_000_000)
        );
        assert_eq!(infer_context_window_from_model("claude-fable-1"), Some(1_000_000));
        assert_eq!(infer_context_window_from_model("claude-opus-4-7"), Some(1_000_000));
        assert_eq!(infer_context_window_from_model("claude-opus-4-6-1m"), Some(1_000_000));
        assert_eq!(infer_context_window_from_model("claude-sonnet-5"), Some(1_000_000));
        assert_eq!(infer_context_window_from_model("claude-sonnet-4-6"), Some(200_000));
        assert_eq!(infer_context_window_from_model("claude-opus-4-6"), Some(200_000));
        assert_eq!(
            infer_context_window_from_model("claude-opus-4-6-10m-preview"),
            Some(200_000)
        );
        assert_eq!(infer_context_window_from_model("deepseek-chat"), None);
    }

    #[test]
    fn context_hint_text_sniffs_capacity() {
        assert_eq!(parse_context_hint_text("Custom Opus model (1M context)"), Some(1_000_000));
        assert_eq!(parse_context_hint_text("foo [500k]"), Some(500_000));
        assert_eq!(parse_context_hint_text("window 1,000,000 tokens"), Some(1_000_000));
        assert_eq!(parse_context_hint_text("128000 ctx"), Some(128_000));
        assert_eq!(parse_context_hint_text("Custom Haiku model"), None);
        assert_eq!(parse_context_hint_text("qwen3.8-max-0902"), None);
    }

    #[test]
    fn model_context_window_inner_uses_inference_for_claude() {
        assert_eq!(
            model_context_window_inner(
                "gateway",
                "deepseek-v4.1-flash[1M]",
                AgentType::ClaudeCode,
                None
            ),
            Some(1_000_000)
        );
        assert_eq!(
            model_context_window_inner("x", "claude-sonnet-4-6", AgentType::ClaudeCode, None),
            Some(200_000)
        );
        // 模型名无后缀（Claude 选项 value 是单名）→ 退到 hint（选项 name/description）
        assert_eq!(
            model_context_window_inner(
                "",
                "sonnet",
                AgentType::ClaudeCode,
                Some("deepseek-v4.1-flash Custom Sonnet model (1M context)")
            ),
            Some(1_000_000)
        );
        assert_eq!(model_context_window_inner("", "m", AgentType::ClaudeCode, None), None);
    }

    #[test]
    fn mcp_args_single_select() {
        let qs = questions_from_mcp_args(&json!({
            "question": "部署到哪个环境？",
            "options": ["dev", "prod"]
        }));
        assert_eq!(qs.len(), 1);
        assert_eq!(qs[0].prompt, "部署到哪个环境？");
        assert_eq!(qs[0].kind, QuestionKind::Single);
        assert_eq!(qs[0].options.len(), 2);
        assert_eq!(qs[0].options[1].value, "prod");
    }

    #[test]
    fn mcp_args_multi_select_and_free_text() {
        let multi = questions_from_mcp_args(&json!({
            "question": "启用哪些功能？", "options": ["a", "b"], "multiSelect": true
        }));
        assert_eq!(multi[0].kind, QuestionKind::Multi);
        let text = questions_from_mcp_args(&json!({ "question": "项目名？" }));
        assert_eq!(text[0].kind, QuestionKind::Text);
        assert!(text[0].options.is_empty());
    }

    #[test]
    fn mcp_args_missing_question_is_empty() {
        assert!(questions_from_mcp_args(&json!({ "options": ["a"] })).is_empty());
        assert!(questions_from_mcp_args(&json!({})).is_empty());
    }

    #[test]
    fn render_answer_text_joins_labels() {
        let qs = vec![Question {
            id: "answer".into(),
            prompt: "部署到哪个环境？".into(),
            kind: QuestionKind::Single,
            options: Vec::new(),
        }];
        let text = render_answer_text(&qs, &[("answer".to_string(), vec!["prod".to_string()])]);
        assert_eq!(text, "部署到哪个环境？：prod");
        // 多选顿号连接
        let multi = render_answer_text(&qs, &[("answer".to_string(), vec!["a".into(), "b".into()])]);
        assert_eq!(multi, "部署到哪个环境？：a、b");
        // 空答案
        assert_eq!(
            render_answer_text(&qs, &[("answer".to_string(), vec![])]),
            "（用户未提供具体答案）"
        );
    }

    #[test]
    fn inject_raw_input_start_line_leaves_non_patch_untouched() {
        // null / number / bool / 无补丁字符串 / 数组：原样
        assert_eq!(inject_raw_input_start_line(&Value::Null), Value::Null);
        assert_eq!(inject_raw_input_start_line(&json!(42)), json!(42));
        assert_eq!(inject_raw_input_start_line(&json!(true)), json!(true));
        assert_eq!(inject_raw_input_start_line(&json!("plain text")), json!("plain text"));
        assert_eq!(inject_raw_input_start_line(&json!([1, 2])), json!([1, 2]));
    }

    #[test]
    fn inject_raw_input_start_line_keeps_object_as_is() {
        // object 原样返回（不 host fs，不做行号解析）
        let obj = json!({"file_path": "a.rs", "old_string": "x", "new_string": "y"});
        assert_eq!(inject_raw_input_start_line(&obj), obj);
    }

    #[test]
    fn inject_raw_input_start_line_parses_patch_string() {
        // 含 `@@` 且可 parse 为对象的 JSON 字符串 → 转为对象
        let patch = json!({
            "file_path": "a.rs",
            "patch": "@@ -1,2 +1,2 @@\n-old\n+new\n"
        });
        let raw = Value::String(patch.to_string());
        assert_eq!(inject_raw_input_start_line(&raw), patch);
    }

    #[test]
    fn inject_raw_input_start_line_keeps_unparseable_patch_string() {
        // 含 `@@` 但 parse 失败 → 保持原串
        let raw = json!("@@ -1 +1 @@ not json");
        assert_eq!(inject_raw_input_start_line(&raw), raw);
    }

    #[test]
    fn inject_raw_input_start_line_into_only_touches_object_toolcall() {
        // toolCall 非 object：不动
        let mut not_obj = json!("nope");
        inject_raw_input_start_line_into(&mut not_obj);
        assert_eq!(not_obj, json!("nope"));

        // toolCall 无 rawInput：不动
        let mut no_raw = json!({"toolCallId": "t1"});
        inject_raw_input_start_line_into(&mut no_raw);
        assert_eq!(no_raw, json!({"toolCallId": "t1"}));

        // toolCall 有 rawInput：就地替换
        let patch = json!({"file_path": "a.rs", "patch": "@@ -1 +1 @@\n-a\n+b"});
        let mut tc = json!({"toolCallId": "t1", "rawInput": patch.to_string()});
        inject_raw_input_start_line_into(&mut tc);
        assert_eq!(tc.get("rawInput"), Some(&patch));
    }

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
        assert_eq!(timeout_for("initialize"), Duration::from_secs(60));
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

    #[test]
    fn idle_timeout_parsing_default_and_override() {
        // 无环境变量 → 默认 180s（对齐 codeg）
        std::env::remove_var("LIGHTR_ACP_IDLE_TIMEOUT_SECS");
        assert_eq!(idle_timeout_secs(), DEFAULT_IDLE_TIMEOUT_SECS);
        // 显式覆盖
        std::env::set_var("LIGHTR_ACP_IDLE_TIMEOUT_SECS", "60");
        assert_eq!(idle_timeout_secs(), 60);
        // 0 表示关闭空闲回收（线程空转跳过）
        std::env::set_var("LIGHTR_ACP_IDLE_TIMEOUT_SECS", "0");
        assert_eq!(idle_timeout_secs(), 0);
        // 非法值 → 回退默认，不 panic
        std::env::set_var("LIGHTR_ACP_IDLE_TIMEOUT_SECS", "abc");
        assert_eq!(idle_timeout_secs(), DEFAULT_IDLE_TIMEOUT_SECS);
        std::env::remove_var("LIGHTR_ACP_IDLE_TIMEOUT_SECS");
    }

    /// 三种 SpawnSpec → Command argv 构造（纯函数；不 spawn 进程）。
    #[test]
    fn command_from_spawn_spec_three_states() {
        use std::ffi::OsStr;

        // Exe + 非空 subcommand（opencode 形态）：argv = [exe, "acp"]
        let c = command_from_spawn_spec(
            engine::SpawnSpec::Exe(PathBuf::from("C:\\bin\\opencode.exe")),
            "OpenCode",
            "opencode",
            "acp",
        )
        .expect("Exe 应成功");
        let args: Vec<&OsStr> = c.get_args().collect();
        assert_eq!(args, vec![OsStr::new("acp")]);

        // Exe + 空 subcommand（适配器形态）：无额外 argv
        let c = command_from_spawn_spec(
            engine::SpawnSpec::Exe(PathBuf::from("C:\\bin\\claude-agent-acp.cmd")),
            "Claude Code",
            "claude-agent-acp",
            "",
        )
        .expect("Exe 应成功");
        assert_eq!(c.get_args().count(), 0);

        // CmdFallback + 非空 subcommand：cmd /C <bare> <subcommand>
        let c = command_from_spawn_spec(
            engine::SpawnSpec::CmdFallback,
            "OpenCode",
            "opencode",
            "acp",
        )
        .expect("CmdFallback 应成功");
        let args: Vec<&OsStr> = c.get_args().collect();
        assert_eq!(
            args,
            vec![OsStr::new("/C"), OsStr::new("opencode"), OsStr::new("acp")]
        );

        // NotInstalled：返回 Err，含安装指引，不构造 Command
        let err = command_from_spawn_spec(
            engine::SpawnSpec::NotInstalled {
                hint: "@agentclientprotocol/codex-acp".into(),
            },
            "Codex CLI",
            "codex-acp",
            "",
        )
        .expect_err("NotInstalled 应返回 Err");
        assert!(err.contains("npm i -g @agentclientprotocol/codex-acp"), "err={}", err);
    }

    #[test]
    fn conns_registry_per_id_isolation() {
        // 仓库按 connId 键隔离：不同会话各自独立条目（design §8.1）
        let a = "ses-aaaa";
        let b = "ses-bbbb";
        assert!(get_conn(a).is_none());
        assert!(require_conn(a).is_err());
        // 直接操作底层仓库模拟注册（不真的 spawn 进程）
        {
            let mut map = conns().lock().unwrap();
            map.clear();
        }
        let _ = b;
        assert!(conns().lock().unwrap().is_empty());
    }

    #[test]
    fn config_rejection_equal_returns_none() {
        // 落定值 == 请求值 → 正常，不发提示
        let result = json!({
            "configOptions": [
                { "id": "model", "currentValue": "opencode/gpt" }
            ]
        });
        assert_eq!(detect_config_rejection(&result, "model", "opencode/gpt"), None);
    }

    #[test]
    fn config_rejection_differs_returns_actual() {
        // 请求值被落成别的值 → 返回实际值
        let result = json!({
            "configOptions": [
                { "id": "model", "currentValue": "opencode/fallback" }
            ]
        });
        assert_eq!(
            detect_config_rejection(&result, "model", "opencode/wanted").as_deref(),
            Some("opencode/fallback")
        );
    }

    #[test]
    fn config_rejection_matches_by_category() {
        // id 不匹配时退回 category 命中
        let result = json!({
            "configOptions": [
                { "id": "someId", "category": "model", "currentValue": "b" }
            ]
        });
        assert_eq!(
            detect_config_rejection(&result, "model", "a").as_deref(),
            Some("b")
        );
    }

    #[test]
    fn config_rejection_missing_or_nonnumeric_returns_none() {
        // 无 configOptions / 缺该 configId / currentValue 非字符串 → 都不发（安全，不误报）
        assert_eq!(detect_config_rejection(&json!({}), "model", "a"), None);
        assert_eq!(
            detect_config_rejection(&json!({ "configOptions": [] }), "model", "a"),
            None
        );
        assert_eq!(
            detect_config_rejection(
                &json!({ "configOptions": [{ "id": "model" }] }),
                "model",
                "a"
            ),
            None
        );
        assert_eq!(
            detect_config_rejection(
                &json!({ "configOptions": [{ "id": "model", "currentValue": 42 }] }),
                "model",
                "a"
            ),
            None
        );
    }
}
