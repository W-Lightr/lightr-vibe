// Agent 检测与配置文件读写命令：仅用 std::process / std::fs，
// 不新增前端 capability、不引入新 crate（design D1）。
// why: 本机 opencode 是 version-fox 的 .cmd shim，CreateProcess 不能直接执行，
// 需要「直接 spawn → cmd /C」回退链（research/opencode-integration.md §1）。

use serde::Serialize;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// 单条外部命令超时：5s（避免检测卡死设置页）
const CMD_TIMEOUT: Duration = Duration::from_secs(5);
/// 配置备份最多保留份数
const MAX_BACKUPS: usize = 5;
/// Windows CREATE_NO_WINDOW：探测命令不弹控制台窗口
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// `agent_detect_opencode` 返回值（camelCase 与前端契约一致，design §2.1）
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectResult {
    pub installed: bool,
    pub version: Option<String>,
    pub bin_path: Option<String>,
    pub platform: String,
    pub acp_ok: bool,
    pub config_path: String,
    pub config_exists: bool,
    /// 检测过程问题明细（超时/退出码异常等），供前端预检 detail 展示
    pub detail: Option<String>,
}

/// `agent_read_opencode_config` 返回值
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigReadResult {
    /// 文件不存在时为 None（前端据此显示引导）
    pub text: Option<String>,
    pub mtime_ms: Option<u64>,
    pub path: String,
}

/// `agent_write_opencode_config` 返回值：写后新 mtime（前端更新冲突校验基准）
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigWriteResult {
    pub mtime_ms: u64,
}

/// `agent_read_env_file` 返回值
#[derive(Serialize)]
pub struct EnvFileResult {
    pub text: Option<String>,
    pub path: String,
}

// ---------- 路径解析 ----------

fn home_dir() -> Result<PathBuf, String> {
    // why: 不引入 dirs crate；Windows 用 USERPROFILE、类 Unix 用 HOME 足够
    #[cfg(target_os = "windows")]
    let key = "USERPROFILE";
    #[cfg(not(target_os = "windows"))]
    let key = "HOME";
    let value = std::env::var(key).map_err(|_| format!("环境变量 {} 缺失", key))?;
    Ok(PathBuf::from(value))
}

fn opencode_config_path() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".config").join("opencode").join("opencode.jsonc"))
}

fn env_file_path() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".lightr-vibe").join("agent-env.json"))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn mtime_ms(path: &Path) -> Result<u64, String> {
    let meta = fs::metadata(path).map_err(|e| format!("读取文件元数据失败: {}", e))?;
    let modified = meta.modified().map_err(|e| format!("读取 mtime 失败: {}", e))?;
    Ok(modified
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis() as u64)
}

/// which 语义解析 opencode 可执行路径（opencode / opencode.cmd / opencode.exe）
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

// ---------- 外部命令执行（超时 + .cmd shim 回退） ----------

struct CmdOutput {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

enum RunError {
    /// spawn 本身失败（Windows 下 .cmd shim 会走到这里，需要 cmd /C 回退）
    Spawn(String),
    Timeout,
    Other(String),
}

impl RunError {
    fn message(&self) -> String {
        match self {
            RunError::Spawn(m) => format!("启动失败: {}", m),
            RunError::Timeout => format!("执行超时（{}s）", CMD_TIMEOUT.as_secs()),
            RunError::Other(m) => m.clone(),
        }
    }
}

/// 带超时执行命令：独立线程读管道避免输出撑满管道死锁；超时 kill 子进程。
fn run_with_timeout(mut cmd: Command) -> Result<CmdOutput, RunError> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let mut child = cmd.spawn().map_err(|e| RunError::Spawn(e.to_string()))?;

    let stdout_handle = child.stdout.take();
    let stderr_handle = child.stderr.take();
    let stdout_reader = std::thread::spawn(move || read_all(stdout_handle));
    let stderr_reader = std::thread::spawn(move || read_all(stderr_handle));

    let deadline = Instant::now() + CMD_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Ok(CmdOutput {
                    code: status.code(),
                    stdout: stdout_reader.join().unwrap_or_default(),
                    stderr: stderr_reader.join().unwrap_or_default(),
                });
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(RunError::Timeout);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                let _ = child.kill();
                return Err(RunError::Other(e.to_string()));
            }
        }
    }
}

fn read_all(mut handle: Option<impl Read>) -> String {
    let mut buf = String::new();
    if let Some(h) = handle.as_mut() {
        let _ = h.read_to_string(&mut buf);
    }
    buf
}

/// 执行 opencode 子命令：直接 spawn 失败（CreateProcess 不能执行 .cmd shim）
/// 时回退 `cmd /C opencode ...`（research §1 回退链）。超时不回退（避免叠加 2×5s）。
fn run_opencode(args: &[&str]) -> Result<CmdOutput, RunError> {
    let mut direct = Command::new("opencode");
    direct.args(args);
    match run_with_timeout(direct) {
        Ok(out) => Ok(out),
        Err(RunError::Spawn(msg)) => {
            #[cfg(windows)]
            {
                let mut wrapped = Command::new("cmd");
                wrapped.arg("/C").arg("opencode").args(args);
                run_with_timeout(wrapped).map_err(|fallback| {
                    RunError::Other(format!(
                        "{}；cmd /C 回退亦失败: {}",
                        msg,
                        fallback.message()
                    ))
                })
            }
            #[cfg(not(windows))]
            Err(RunError::Spawn(msg))
        }
        Err(e) => Err(e),
    }
}

/// 从输出中提取第一个「数字.数字.数字」形态的 semver 子串（不引入 regex）
fn extract_version(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() || chars[i] == '.' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let token: String = chars[start..i].iter().collect();
            let parts: Vec<&str> = token.split('.').collect();
            if parts.len() >= 3
                && parts[..3]
                    .iter()
                    .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
            {
                return Some(parts[..3].join("."));
            }
        } else {
            i += 1;
        }
    }
    None
}

// ---------- 备份与原子写 ----------

/// 备份当前文件为 `<name>.bak-<ts>`，按名称排序清理只保留最近 MAX_BACKUPS 份
fn backup_file(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("配置文件名不合法")?;
    let dir = path.parent().ok_or("配置目录不合法")?;

    let bak = path.with_file_name(format!("{}.bak-{}", file_name, now_ms()));
    fs::copy(path, &bak).map_err(|e| format!("备份失败: {}", e))?;

    let prefix = format!("{}.bak-", file_name);
    let mut backups: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| format!("读取备份目录失败: {}", e))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with(&prefix))
                .unwrap_or(false)
        })
        .collect();
    // 备份名内嵌毫秒时间戳，字典序即时间序
    backups.sort();
    while backups.len() > MAX_BACKUPS {
        if let Some(oldest) = backups.first() {
            let _ = fs::remove_file(oldest);
        }
        backups.remove(0);
    }
    Ok(())
}

/// 原子写：先写同目录临时文件再 rename，避免写一半崩溃留下损坏配置
fn atomic_write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
    }
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("目标文件名不合法")?;
    let tmp = path.with_file_name(format!("{}.tmp-{}", file_name, now_ms()));
    fs::write(&tmp, text).map_err(|e| format!("写入临时文件失败: {}", e))?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("重命名写入失败: {}", e)
    })
}

// ---------- Tauri commands ----------

/// 检测 opencode：PATH 解析 + --version + acp --help（仅探测不启动服务）。
/// why async + spawn_blocking: 检测最多串行两条外部命令（各 5s 超时），
/// 不能阻塞主线程/async runtime。
#[tauri::command]
pub async fn agent_detect_opencode() -> DetectResult {
    match tauri::async_runtime::spawn_blocking(detect_opencode_inner).await {
        Ok(result) => result,
        Err(e) => DetectResult {
            installed: false,
            version: None,
            bin_path: resolve_bin(),
            platform: format!("{} / {}", std::env::consts::OS, std::env::consts::ARCH),
            acp_ok: false,
            config_path: opencode_config_path()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            config_exists: false,
            detail: Some(format!("检测任务执行失败: {}", e)),
        },
    }
}

fn detect_opencode_inner() -> DetectResult {
    let bin_path = resolve_bin();
    let platform = format!("{} / {}", std::env::consts::OS, std::env::consts::ARCH);
    let (config_path, config_exists) = match opencode_config_path() {
        Ok(p) => {
            let exists = p.exists();
            (p.to_string_lossy().into_owned(), exists)
        }
        Err(_) => (String::new(), false),
    };

    let mut problems: Vec<String> = Vec::new();
    let mut version: Option<String> = None;

    // 1) 版本探测
    match run_opencode(&["--version"]) {
        Ok(out) if out.code == Some(0) => {
            version = extract_version(&out.stdout).or_else(|| extract_version(&out.stderr));
            if version.is_none() {
                problems.push("--version 输出中未解析到版本号".into());
            }
        }
        Ok(out) => problems.push(format!("--version 退出码异常（{:?}）", out.code)),
        Err(e) => problems.push(format!("--version {}", e.message())),
    }

    // 2) ACP 能力探测（退出码 0 = 可用；仅 --help，不启动服务）
    let acp_ok = match run_opencode(&["acp", "--help"]) {
        Ok(out) => out.code == Some(0),
        Err(e) => {
            problems.push(format!("acp --help {}", e.message()));
            false
        }
    };

    // 已安装 = PATH 解析到可执行文件，或版本命令跑通并解析出 semver
    let installed = bin_path.is_some() || version.is_some();

    DetectResult {
        installed,
        version,
        bin_path,
        platform,
        acp_ok,
        config_path,
        config_exists,
        detail: if problems.is_empty() {
            None
        } else {
            Some(problems.join("；"))
        },
    }
}

/// 读 opencode.jsonc 原文 + mtime（前端负责 JSONC 解析与补丁生成，design D2）
#[tauri::command]
pub fn agent_read_opencode_config() -> Result<ConfigReadResult, String> {
    let path = opencode_config_path()?;
    let path_str = path.to_string_lossy().into_owned();
    if !path.exists() {
        return Ok(ConfigReadResult {
            text: None,
            mtime_ms: None,
            path: path_str,
        });
    }
    let text = fs::read_to_string(&path).map_err(|e| format!("读取配置失败: {}", e))?;
    let mtime = mtime_ms(&path)?;
    Ok(ConfigReadResult {
        text: Some(text),
        mtime_ms: Some(mtime),
        path: path_str,
    })
}

/// 写回 opencode.jsonc：mtime 冲突校验 → .bak-<ts> 备份（保留 5 份）→ 原子写。
/// expected_mtime 为 None 表示读取时文件不存在；不一致统一返回 Err("conflict")，
/// 前端据此提示「配置已被外部修改」并重新加载（research §5）。
#[tauri::command]
pub fn agent_write_opencode_config(
    text: String,
    expected_mtime: Option<u64>,
) -> Result<ConfigWriteResult, String> {
    let path = opencode_config_path()?;
    let exists = path.exists();
    match (expected_mtime, exists) {
        // 读时不存在、写时出现了 → 外部新建，视为冲突
        (None, true) => return Err("conflict".into()),
        // 读时存在、写时已被删除 → 状态漂移，视为冲突
        (Some(_), false) => return Err("conflict".into()),
        (Some(expected), true) => {
            let current = mtime_ms(&path)?;
            if current != expected {
                return Err("conflict".into());
            }
        }
        // (None, false)：首次创建，无需校验
        _ => {}
    }

    backup_file(&path)?;
    atomic_write(&path, &text)?;
    Ok(ConfigWriteResult {
        mtime_ms: mtime_ms(&path)?,
    })
}

/// 读应用级环境变量文件 ~/.lightr-vibe/agent-env.json（不存在返回 text: null）
#[tauri::command]
pub fn agent_read_env_file() -> Result<EnvFileResult, String> {
    let path = env_file_path()?;
    let path_str = path.to_string_lossy().into_owned();
    let text = if path.exists() {
        Some(fs::read_to_string(&path).map_err(|e| format!("读取环境变量文件失败: {}", e))?)
    } else {
        None
    };
    Ok(EnvFileResult { text, path: path_str })
}

/// 写应用级环境变量文件（目录不存在自动创建；纯 JSON 无注释问题，直接原子写）
#[tauri::command]
pub fn agent_write_env_file(text: String) -> Result<(), String> {
    let path = env_file_path()?;
    atomic_write(&path, &text)
}
