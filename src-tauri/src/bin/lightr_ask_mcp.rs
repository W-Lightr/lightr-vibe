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
}

fn parse_args() -> Result<Args, String> {
    let mut socket = None;
    let mut token = None;
    let mut conn_id = None;
    let mut parent_pid = None;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--socket" => socket = it.next(),
            "--token" => token = Some(it.next().unwrap_or_default()),
            "--conn-id" => conn_id = Some(it.next().unwrap_or_default()),
            "--parent-pid" => parent_pid = it.next().and_then(|v| v.parse::<u32>().ok()),
            _ => {}
        }
    }
    Ok(Args {
        socket: socket.ok_or("missing --socket")?,
        token: token.ok_or("missing --token")?,
        conn_id: conn_id.ok_or("missing --conn-id")?,
        parent_pid,
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
        "tools/list" => Some(ok(
            &id,
            serde_json::json!({ "tools": [ tool_def() ] }),
        )),
        "tools/call" => {
            let params = msg.get("params").cloned().unwrap_or(serde_json::Value::Null);
            let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if name != "ask_user_question" {
                return Some(err(&id, -32602, &format!("unknown tool: {name}")));
            }
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or(serde_json::json!({}));
            match ask_via_socket(args, &arguments) {
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
fn tool_def() -> serde_json::Value {
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

/// 经 socket 把问题回传主进程，阻塞等答案。
fn ask_via_socket(
    args: &Args,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut stream = TcpStream::connect(&args.socket)
        .map_err(|e| format!("连接主进程失败（{}）：{e}", args.socket))?;
    let req = serde_json::json!({
        "kind": "ask",
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
