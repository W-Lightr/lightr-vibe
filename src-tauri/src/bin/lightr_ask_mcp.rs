//! `lightr-ask-mcp` — 伴生 stdio MCP server（每会话一个，由引擎按 `session/new.mcpServers`
//! 拉起）。只暴露一个工具 `ask_user_question`：模型调用它 → 本进程经**回环 TCP socket**
//! 把问题回传给主进程 → 主进程弹提问卡 → 用户答 → 经 socket 回传 → 本进程把答案作为
//! MCP 工具结果返回给模型。
//!
//! 设计（design.md §2）：本进程**只做透传**——不解析业务语义，不依赖主 crate 类型；
//! 仅负责 MCP 握手、`tools/list`、`tools/call` 的 socket 往返。全部 std（无 tokio），
//! 与主进程（tauri/std 线程模型）一致。
//!
//! 用法（由主进程注入 `session/new.mcpServers` 的 args）：
//!   lightr-ask-mcp --socket 127.0.0.1:<port> --token <secret> --conn-id <connId> [--parent-pid <pid>]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("lightr-ask-mcp: {e}");
            return ExitCode::FAILURE;
        }
    };

    let stdin = std::io::stdin();
    let reader = BufReader::new(stdin.lock());
    let mut stdout = std::io::stdout();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break, // stdin 关闭 → 退出（引擎会话结束）
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let msg: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue, // 非 JSON 行忽略
        };
        if let Some(resp) = handle_line(&msg, &args) {
            if writeln!(stdout, "{}", resp).is_err() || stdout.flush().is_err() {
                break;
            }
        }
    }

    let _ = &args.parent_pid; // 会话由引擎管理；父 pid 兜底仅用于异常自退（见下）
    ExitCode::SUCCESS
}

/// 启动参数（由主进程注入 args）。
struct Args {
    socket: String,
    token: String,
    conn_id: String,
    parent_pid: Option<u32>,
    delegation_enabled: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut socket = None;
    let mut token = None;
    let mut conn_id = None;
    let mut parent_pid = None;
    let mut delegation_enabled = false;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--socket" => socket = it.next(),
            "--token" => token = Some(it.next().unwrap_or_default()),
            "--conn-id" => conn_id = Some(it.next().unwrap_or_default()),
            "--parent-pid" => parent_pid = it.next().and_then(|v| v.parse::<u32>().ok()),
            "--delegation-enabled" => delegation_enabled = it.next().is_some_and(|v| v == "true"),
            _ => {}
        }
    }
    Ok(Args {
        socket: socket.ok_or("missing --socket")?,
        token: token.ok_or("missing --token")?,
        conn_id: conn_id.ok_or("missing --conn-id")?,
        parent_pid,
        delegation_enabled,
    })
}

/// 处理一条入站 MCP 消息；返回需要写回 stdout 的响应（None = notification，不回）。
fn handle_line(msg: &serde_json::Value, args: &Args) -> Option<serde_json::Value> {
    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let id = msg.get("id").cloned();

    // notification（无 id）：cancel 等忽略（本进程的 ask 是同步阻塞，无并发取消）
    let id = id?;

    match method {
        "initialize" => Some(ok(
            &id,
            serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "lightr-ask", "version": "0.1.0" }
            }),
        )),
        "tools/list" => {
            let mut tools = vec![ask_tool_def()];
            if args.delegation_enabled { tools.extend(delegation_tool_defs()); }
            Some(ok(&id, serde_json::json!({ "tools": tools })))
        }
        "tools/call" => {
            let params = msg.get("params").cloned().unwrap_or(serde_json::Value::Null);
            let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if name != "ask_user_question" && !(args.delegation_enabled && matches!(name, "delegate_to_agent" | "get_delegation_status" | "cancel_delegation")) {
                return Some(err(&id, -32602, &format!("unknown tool: {name}")));
            }
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or(serde_json::json!({}));
            let kind = match name {
                "ask_user_question" => "ask",
                "delegate_to_agent" => "delegate",
                "get_delegation_status" => "status",
                _ => "cancel",
            };
            match call_via_socket(args, kind, &arguments) {
                Ok(result) => Some(ok(&id, result)),
                Err(e) => Some(ok(
                    &id,
                    // 工具级错误（isError）而非协议错误：模型能读懂并自行继续
                    serde_json::json!({
                        "content": [{ "type": "text", "text": format!("提问失败：{e}") }],
                        "isError": true
                    }),
                )),
            }
        }
        _ => Some(err(&id, -32601, &format!("method not found: {method}"))),
    }
}

/// `ask_user_question` 的工具定义（schema 对齐 codeg/Claude Code 形态，本期最小）。
fn ask_tool_def() -> serde_json::Value {
    serde_json::json!({
        "name": "ask_user_question",
        "description": "向用户提出一个问题并等待回答。当需要用户澄清需求、在多个方案中选择、或确认关键决策时使用。options 为空表示自由文本回答。",
        "inputSchema": {
            "type": "object",
            "properties": {
                "question": { "type": "string", "description": "要问用户的问题" },
                "options": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "可选项（单选/多选）；为空则是自由文本输入"
                },
                "multiSelect": { "type": "boolean", "description": "是否允许多选（默认 false）" },
                "header": { "type": "string", "description": "简短标题（可选）" }
            },
            "required": ["question"]
        }
    })
}

fn delegation_tool_defs() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "delegate_to_agent",
            "description": "在 Lightr 内启动一个独立子代理执行子任务，立即返回 task_id；子代理在自己的会话里异步工作，不阻塞当前对话。\n\n【何时使用】任何需要子代理的场景：拆分出的子任务、并行探索、大范围目录/代码检索、独立的长任务。\n\n【强制约定】\n1. 在本应用内创建子任务必须使用本工具。禁止使用引擎内置的子任务工具，例如 Claude Code 的 Agent / Task 工具、opencode 的 task 工具、Codex 的协作代理（collab / spawn）工具。\n2. 内置子任务工具不会出现在 Lightr 的「子任务」面板中，用户看不到进度、无法查看子会话，任务也不受 Lightr 生命周期管理；只有本工具创建的任务会展示在右侧「子任务」页签。\n3. 同一个任务只委派一次。需要等待结果时用 get_delegation_status 轮询，不要因为等待就重复调用本工具（会产生多张重复的委派卡）。\n\n【参数】agent_type：执行引擎（open_code / claude_code / codex）；task：完整、自包含的任务说明（子代理看不到当前聊天历史）；working_dir：可选，必须位于当前项目目录内。\n\n【返回】{ taskId, status, childSessionId, ... }。taskId 用于 get_delegation_status 查询和 cancel_delegation 取消。",
            "inputSchema": { "type": "object", "required": ["agent_type", "task"], "properties": {
                "agent_type": { "type": "string", "description": "已安装的 ACP 引擎：open_code、claude_code 或 codex" },
                "task": { "type": "string", "description": "完整任务说明；子代理看不到当前聊天历史" },
                "working_dir": { "type": "string", "description": "可选，必须位于父会话项目目录内" }
            }}
        }),
        serde_json::json!({
            "name": "get_delegation_status",
            "description": "查询由 delegate_to_agent 创建的一个或多个子任务的状态和结果摘要。等待子任务时用本工具轮询（间隔几秒），不要重复调用 delegate_to_agent 创建同一任务。",
            "inputSchema": { "type": "object", "required": ["task_ids"], "properties": {
                "task_ids": { "type": "array", "items": { "type": "string" } }
            }}
        }),
        serde_json::json!({
            "name": "cancel_delegation",
            "description": "取消由 delegate_to_agent 创建且仍在运行的子任务。",
            "inputSchema": { "type": "object", "required": ["task_id"], "properties": { "task_id": { "type": "string" } }}
        }),
    ]
}

/// 经 socket 把问题回传主进程，阻塞等答案。
fn call_via_socket(
    args: &Args,
    kind: &str,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut stream = TcpStream::connect(&args.socket)
        .map_err(|e| format!("连接主进程失败（{}）：{e}", args.socket))?;
    let req = serde_json::json!({
        "kind": kind,
        "token": args.token,
        "connId": args.conn_id,
        "arguments": arguments
    });
    let mut line = serde_json::to_string(&req).map_err(|e| e.to_string())?;
    line.push('\n');
    stream
        .write_all(line.as_bytes())
        .map_err(|e| format!("发送失败：{e}"))?;
    stream.flush().ok();

    // 读一行应答（主进程答完即关闭）
    let mut reader = BufReader::new(stream);
    let mut resp_line = String::new();
    reader
        .read_line(&mut resp_line)
        .map_err(|e| format!("读应答失败：{e}"))?;
    if resp_line.trim().is_empty() {
        return Err("主进程未返回（连接关闭）".to_string());
    }
    let resp: serde_json::Value =
        serde_json::from_str(resp_line.trim()).map_err(|e| format!("应答解析失败：{e}"))?;

    if kind != "ask" {
        let ok = resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
        let text = if ok {
            resp.get("result").map(|v| v.to_string()).unwrap_or_else(|| "{}".into())
        } else {
            format!("委派操作失败：{}", resp.get("error").and_then(|v| v.as_str()).unwrap_or("unknown"))
        };
        return Ok(serde_json::json!({ "content": [{ "type": "text", "text": text }], "structuredContent": resp.get("result").cloned().unwrap_or(serde_json::Value::Null), "isError": !ok }));
    }
    let declined = resp.get("declined").and_then(|v| v.as_bool()).unwrap_or(false);
    if declined {
        return Ok(serde_json::json!({
            "content": [{ "type": "text", "text": "用户跳过了该问题。" }],
            "isError": false
        }));
    }
    let answer = resp
        .get("answer")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Ok(serde_json::json!({
        "content": [{ "type": "text", "text": answer }],
        "isError": false
    }))
}

fn ok(id: &serde_json::Value, result: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn err(id: &serde_json::Value, code: i64, message: &str) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0", "id": id,
        "error": { "code": code, "message": message }
    })
}
