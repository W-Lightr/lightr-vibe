// 引擎静态元数据注册表（照 codeg `acp/registry.rs` 裁剪）。
// 新增引擎 = 加一个 `AgentType` 变体 + 一条 `get_agent_meta` match 臂；所有
// spawn/检测/配置路径/context-window 规则都从 `AcpAgentMeta` 数据取出，避免代码分支膨胀。
// 现有 opencode / claude_code / codex 三条（后两者为 ExternalAdapter 分发形态）。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::agent_type::{AgentType, BUILTIN_AGENT_TYPES};

/// context-window 的来源策略（各引擎元数据存放位置不同）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextWindowSource {
    /// opencode：用户配置（config.json/opencode.json/opencode.jsonc）→ models.dev 缓存目录
    OpenCodeConfigAndCatalog,
    /// 无已知来源：返回 None（前端保持既有分母，不臆造）。claude/codex 当前使用。
    None,
}

/// 一个引擎的静态 launch 元数据。
#[derive(Debug, Clone, Copy)]
pub struct AcpAgentMeta {
    /// 该 meta 对应的引擎；`get_agent_meta` 保证与查表键一致（单测校验）。
    /// 生产代码从查表键即可推知，字段供测试与将来按 meta 反查用。
    #[allow(dead_code)]
    pub agent_type: AgentType,
    /// 显示名（UI 文案）
    pub name: &'static str,
    pub description: &'static str,
    pub launch: LaunchSpec,
}

/// 引擎的分发形态（照 codeg AgentDistribution 裁剪，仅保留 launch 必需两态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchKind {
    /// 直接启动 PATH 上的可执行/子命令（opencode：`opencode acp`）。
    /// 语义同现状：resolve_spawn 走 bin_candidates + shim 回退。
    DirectCommand,
    /// 外部适配器命令（用户自装）：命令真身在 PATH 或 npm 全局 bin。
    /// spawn 时：PATH 命令 → npm 全局 bin 命令；全未命中 → 报「未安装适配器」。
    ExternalAdapter,
}

/// 引擎专属的启动/检测/配置声明式规则。
#[derive(Debug, Clone, Copy)]
pub struct LaunchSpec {
    /// resolve_bin 候选（含 .exe/.cmd/.bat；Windows 下按此顺序在 PATH 中查找）
    pub bin_candidates: &'static [&'static str],
    /// ACP 子命令（spawn 时追加到 bin 后，如 `opencode acp`）；
    /// ExternalAdapter 为空串（适配器命令本身即 ACP 入口，无子命令）
    pub acp_subcommand: &'static str,
    /// spawn 时移除的环境变量（泄漏会让子进程上游鉴权失败）
    pub env_strip: &'static [&'static str],
    /// 版本检测命令参数（如 `--version`）
    pub detect_args: &'static [&'static str],
    /// ACP 能力检测命令参数（如 `acp --help`；退出码 0 = 可用）
    pub acp_probe_args: &'static [&'static str],
    /// 配置文件路径规则（相对 home 的片段）；None = 无独立配置文件
    pub config_path: Option<&'static [&'static str]>,
    /// context-window 查找策略
    pub context_window: ContextWindowSource,
    /// 分发形态（DirectCommand = 现状；ExternalAdapter = 用户自装适配器）
    pub kind: LaunchKind,
    /// ExternalAdapter 专用：npm 包规格（如 `@agentclientprotocol/claude-agent-acp`），
    /// **仅用于错误提示里的安装命令指引**（`npm i -g <package>`），不触发任何下载。
    /// DirectCommand 时为 None。
    pub install_hint: Option<&'static str>,
}

/// opencode 声明（承载 acp.rs:133-206、agent.rs:76/103/193/201、acp.rs:943-1079 的常量）。
const OPENCODE_META: AcpAgentMeta = AcpAgentMeta {
    agent_type: AgentType::OpenCode,
    name: "OpenCode",
    description: "支持 ACP（Agent Client Protocol）的开源编码 Agent",
    launch: LaunchSpec {
        // Windows 需覆盖 version-fox 的 .cmd/.bat shim；类 Unix 只有裸名
        bin_candidates: if cfg!(windows) {
            &["opencode.exe", "opencode.cmd", "opencode.bat", "opencode"]
        } else {
            &["opencode"]
        },
        acp_subcommand: "acp",
        // 父进程可能从另一个运行中的 opencode 实例继承 server 凭据，泄漏给子进程会让
        // Zen 免费通道上游鉴权失败（-32603 Invalid credential）
        env_strip: &["OPENCODE_SERVER_USERNAME", "OPENCODE_SERVER_PASSWORD"],
        detect_args: &["--version"],
        acp_probe_args: &["acp", "--help"],
        config_path: Some(&[".config", "opencode", "opencode.jsonc"]),
        context_window: ContextWindowSource::OpenCodeConfigAndCatalog,
        kind: LaunchKind::DirectCommand,
        install_hint: None,
    },
};

/// Claude Code 声明：原生 CLI 不说 ACP，经官方适配器 `claude-agent-acp`（用户自装）桥接。
/// 分发形态 = ExternalAdapter：命令真身在 PATH 或 npm 全局 bin，我们只解析、不打包、不下载。
/// 字段值来自 Step 0 实测（research §7）：支持 `--version`；`--help` 不可靠故不用；
/// 无独立可编辑配置文件（凭据由厂商 CLI 托管于 ~/.claude）。
const CLAUDE_CODE_META: AcpAgentMeta = AcpAgentMeta {
    agent_type: AgentType::ClaudeCode,
    name: "Claude Code",
    description: "Anthropic Claude 的 ACP 适配器（用户自装 claude-agent-acp）",
    launch: LaunchSpec {
        // npm 全局装的适配器命令在 Windows 上是 .cmd/.ps1 shim（无裸名/无 .exe）
        bin_candidates: if cfg!(windows) {
            &[
                "claude-agent-acp.cmd",
                "claude-agent-acp.exe",
                "claude-agent-acp.bat",
                "claude-agent-acp",
            ]
        } else {
            &["claude-agent-acp"]
        },
        acp_subcommand: "", // 适配器命令即 ACP 入口
        env_strip: &[],     // 实测：无已知需清洗变量
        detect_args: &["--version"],
        acp_probe_args: &["--version"], // 实测：--help 无输出，用 --version 当探针
        config_path: None,              // 无独立可编辑配置文件（D-cfg）
        context_window: ContextWindowSource::None,
        kind: LaunchKind::ExternalAdapter,
        install_hint: Some("@agentclientprotocol/claude-agent-acp"),
    },
};

/// Codex 声明：同 Claude，经官方适配器 `codex-acp`（用户自装）桥接。
/// 字段值来自 Step 0 实测（research §7）：支持 `--version`；`--help` 会崩溃故不用。
const CODEX_META: AcpAgentMeta = AcpAgentMeta {
    agent_type: AgentType::Codex,
    name: "Codex CLI",
    description: "OpenAI Codex 的 ACP 适配器（用户自装 codex-acp）",
    launch: LaunchSpec {
        bin_candidates: if cfg!(windows) {
            &[
                "codex-acp.cmd",
                "codex-acp.exe",
                "codex-acp.bat",
                "codex-acp",
            ]
        } else {
            &["codex-acp"]
        },
        acp_subcommand: "",
        env_strip: &[],
        detect_args: &["--version"],
        acp_probe_args: &["--version"],
        config_path: None,
        context_window: ContextWindowSource::None,
        kind: LaunchKind::ExternalAdapter,
        install_hint: Some("@agentclientprotocol/codex-acp"),
    },
};

/// 静态查表（infallible；枚举穷尽 match）。
pub fn get_agent_meta(agent_type: AgentType) -> AcpAgentMeta {
    match agent_type {
        AgentType::OpenCode => OPENCODE_META,
        AgentType::ClaudeCode => CLAUDE_CODE_META,
        AgentType::Codex => CODEX_META,
    }
}

/// 列表下发形状（照 codeg AcpAgentInfo 裁剪；camelCase 与前端契约一致）。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AcpAgentInfo {
    /// wire 序列化（AgentType 自带 Serialize → as_wire）
    pub agent_type: AgentType,
    pub name: String,
    pub description: String,
    /// 是否启用（预留；本期恒 true）
    pub enabled: bool,
    /// 排序权重（预留；本期恒 0，前端按 sortOrder + name 排序）
    pub sort_order: i32,
    /// 是否检测通过（PATH 解析到可执行文件）
    pub available: bool,
    pub installed_version: Option<String>,
}

/// 列出所有内置引擎；available/installed_version 由实机检测填充。
/// `available` 与 `agent_detect` 口径一致（PATH **或** npm 全局 bin 可解析，D5）：
/// 仅装在 npm 全局 bin 的外部适配器（claude/codex）不会在新建弹窗里被误标「未安装」。
pub fn list_agents() -> Vec<AcpAgentInfo> {
    BUILTIN_AGENT_TYPES
        .iter()
        .map(|&agent_type| {
            let meta = get_agent_meta(agent_type);
            let available = resolve_bin_for_detect(&meta).is_some();
            AcpAgentInfo {
                agent_type,
                name: meta.name.to_string(),
                description: meta.description.to_string(),
                enabled: true,
                sort_order: 0,
                available,
                installed_version: None,
            }
        })
        .collect()
}

// ---------- 引擎化的 bin 解析（收敛 acp.rs / agent.rs 两处重复实现） ----------

/// 裸可执行名（去掉 .exe/.cmd/.bat 扩展）：供 `cmd /C` 回退交给 cmd 走 PATHEXT 解析。
/// 取 bin_candidates 首个（Windows 下如 "opencode.exe" → "opencode"）。
/// 候选名均为 'static 字面量，strip_suffix 保持 'static 生命周期。
pub fn bare_bin_name(meta: &AcpAgentMeta) -> &'static str {
    let first = meta.launch.bin_candidates.first().copied().unwrap_or("");
    first
        .strip_suffix(".exe")
        .or_else(|| first.strip_suffix(".cmd"))
        .or_else(|| first.strip_suffix(".bat"))
        .unwrap_or(first)
}

/// which 语义解析引擎可执行路径：按 meta.bin_candidates 顺序扫描 PATH。
pub fn resolve_bin(meta: &AcpAgentMeta) -> Option<String> {
    let path_var = std::env::var("PATH").ok()?;
    for dir in std::env::split_paths(&path_var) {
        for name in meta.launch.bin_candidates {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
    }
    None
}

/// 从 Windows .cmd shim 文本提取真身 exe 路径（version-fox 形态：
/// `"%dp0%\node_modules\opencode-ai\bin\opencode.exe" %*`，probe §0）。
/// 支持 `%dp0%` 与 `%~dp0` 两种占位符；替换后统一为单个路径分隔符。
///
/// **不提取 `node`/`node.exe` 解释器**（npm 形态 shim：`"%_prog%" "…\dist\index.js" %*`
/// 或 `"%dp0%\node.exe" "…\index.js"`）：那种 shim 的真身是 `node <脚本>`，脚本在
/// **另一个参数**里，只取 node.exe 会丢掉脚本 → spawn 裸 node，收到 JSON-RPC 行当作 JS
/// 抛 SyntaxError、initialize 永不响应（表现为 `timeout:10s initialize`）。此时返回
/// `None` → 上层走 `cmd /C` 回退（由 cmd 正确执行 shim，实测 ~1.8s 完成握手）。
pub fn extract_exe_from_shim(shim_text: &str, shim_dir: &str) -> Option<String> {
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
                    // node 解释器：真身在脚本参数里，不能只取解释器（见文档注释）。
                    if is_node_interpreter(&resolved) {
                        return None;
                    }
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

/// 该路径是否为 node/node.exe 解释器（按文件名判定，忽略大小写）。
/// 出现即说明 shim 是「解释器 + 脚本」两段式，不能只取解释器。
fn is_node_interpreter(path: &str) -> bool {
    let name = path
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase();
    name == "node" || name == "node.exe"
}

/// 引擎 spawn 解析结果。
pub enum SpawnSpec {
    /// 直接 spawn 真身 exe（probe §0 路线 1：长驻双向 stdio 进程最干净）
    Exe(PathBuf),
    /// 末选回退：`cmd /C <bin> <subcommand>`（tauri-bridge Gotcha：.cmd 无法 CreateProcess）
    CmdFallback,
    /// ExternalAdapter 全未命中：适配器未安装，携带安装指引，spawn 时报错（不 spawn 进程）。
    NotInstalled { hint: String },
}

/// npm 全局 bin 目录形态：Windows 下即 `<prefix>`，类 Unix 下为 `<prefix>/bin`。
/// 独立纯函数以便单测（不真跑 npm）。
fn npm_bin_dir_from_prefix(prefix: &str, windows: bool) -> PathBuf {
    let base = PathBuf::from(prefix);
    if windows {
        base
    } else {
        base.join("bin")
    }
}

/// 解析 npm 全局 bin 目录：跑 `npm prefix -g` 取前缀 → `<prefix>`（Windows）
/// 或 `<prefix>/bin`（类 Unix）。**进程内只查一次**（acp_list_agents / agent_detect /
/// resolve_spawn 三处都可能触发）；失败同样缓存 None，避免每次调用都起一次 npm。
/// 失败返回 None（不报错，交由上层报未安装）。**纯解析，不做任何下载/安装**（AC11）。
fn resolve_npm_global_bin_dir() -> Option<PathBuf> {
    static NPM_BIN_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    NPM_BIN_DIR
        .get_or_init(resolve_npm_global_bin_dir_uncached)
        .clone()
}

/// 真正跑 `npm prefix -g`（仅在 OnceLock 首次初始化时执行一次）。
fn resolve_npm_global_bin_dir_uncached() -> Option<PathBuf> {
    let mut cmd = std::process::Command::new("npm");
    cmd.arg("prefix").arg("-g");
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let child = cmd.spawn().ok()?;
    // npm 可能因网络/配置卡住：用 wait_with_output 前先设短超时需自旋；此处走简单 wait
    // （`npm prefix -g` 为本地查询，正常毫秒级返回）。
    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }
    let prefix = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if prefix.is_empty() {
        return None;
    }
    Some(npm_bin_dir_from_prefix(&prefix, cfg!(windows)))
}

/// 在给定目录按 bin_candidates 顺序找命令真身（is_file 判据）。
fn candidate_in_dir(dir: &Path, bin_candidates: &[&str]) -> Option<String> {
    for name in bin_candidates {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    None
}

/// 在 npm 全局 bin 目录按 bin_candidates 找适配器命令真身（.cmd/.exe/裸名）。
fn resolve_bin_in_npm_dir(meta: &AcpAgentMeta) -> Option<String> {
    let dir = resolve_npm_global_bin_dir()?;
    candidate_in_dir(&dir, meta.launch.bin_candidates)
}

/// 检测用 resolve：PATH 命中优先；ExternalAdapter 再退 npm 全局 bin。
/// DirectCommand 只走 PATH（保持现状语义）。
pub fn resolve_bin_for_detect(meta: &AcpAgentMeta) -> Option<String> {
    if let Some(bin) = resolve_bin(meta) {
        return Some(bin);
    }
    match meta.launch.kind {
        LaunchKind::ExternalAdapter => resolve_bin_in_npm_dir(meta),
        LaunchKind::DirectCommand => None,
    }
}

/// 把已解析到的路径转成 SpawnSpec：.cmd/.bat shim 尝试提取真身 exe，裸 exe 直接 spawn，
/// 提取失败/未知扩展 → `cmd /C` 回退（DirectCommand 语义）。供 PATH 与 npm 两处复用。
fn spawn_spec_from_path(bin: &str) -> SpawnSpec {
    let path = Path::new(bin);
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
            // shim 未解析出真身：交给 cmd /C 回退（DirectCommand 场景）
            SpawnSpec::CmdFallback
        }
        _ => {
            if path.is_file() {
                SpawnSpec::Exe(path.to_path_buf())
            } else {
                SpawnSpec::CmdFallback
            }
        }
    }
}

/// spawn 解析链（按 meta.launch.kind 分派，**不引用任何引擎名**）：
/// - DirectCommand：PATH bin_candidates → shim 提取真身 → cmd /C 末选（现状不变）。
/// - ExternalAdapter：PATH → npm 全局 bin → 全未命中 NotInstalled（携带安装指引）。
pub fn resolve_spawn(meta: &AcpAgentMeta) -> SpawnSpec {
    match meta.launch.kind {
        LaunchKind::DirectCommand => {
            if let Some(bin) = resolve_bin(meta) {
                return spawn_spec_from_path(&bin);
            }
            SpawnSpec::CmdFallback
        }
        LaunchKind::ExternalAdapter => {
            // 1) PATH 上已装的适配器命令
            if let Some(bin) = resolve_bin(meta) {
                return spawn_spec_from_path(&bin);
            }
            // 2) npm 全局 bin 下已装的适配器命令
            if let Some(bin) = resolve_bin_in_npm_dir(meta) {
                return spawn_spec_from_path(&bin);
            }
            // 3) 全未命中：适配器未安装，携带 `npm i -g <package>` 指引（不下载）
            let hint = meta
                .launch
                .install_hint
                .map(|p| p.to_string())
                .unwrap_or_else(|| meta.name.to_string());
            SpawnSpec::NotInstalled { hint }
        }
    }
}

// ---------- 全局设置（默认引擎）持久化 ----------
//
// 命名空间硬规则（tauri-bridge spec）：仅写 ~/.lightr-vibe/lightr-vibe/settings.json，
// 不触碰根下其他应用的既有数据。默认引擎存 wire 值（如 "open_code"）。

/// settings.json 形状：`{ "defaultEngineId": "open_code" }`
#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SettingsFile {
    default_engine_id: Option<String>,
}

fn home_dir() -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    let key = "USERPROFILE";
    #[cfg(not(target_os = "windows"))]
    let key = "HOME";
    let value = std::env::var(key).map_err(|_| format!("环境变量 {} 缺失", key))?;
    Ok(PathBuf::from(value))
}

/// 全局设置文件路径：~/.lightr-vibe/lightr-vibe/settings.json
fn settings_path() -> Result<PathBuf, String> {
    Ok(home_dir()?
        .join(".lightr-vibe")
        .join("lightr-vibe")
        .join("settings.json"))
}

/// 读取全局设置；文件缺失/损坏 → 默认值（不报错）。
fn read_settings() -> SettingsFile {
    settings_path()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|text| serde_json::from_str::<SettingsFile>(&text).ok())
        .unwrap_or_default()
}

/// 写全局设置（目录自动创建；纯 JSON 原子写）。
fn write_settings(settings: &SettingsFile) -> Result<(), String> {
    let path = settings_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建设置目录失败: {}", e))?;
    }
    let text = serde_json::to_string_pretty(settings).map_err(|e| format!("序列化设置失败: {}", e))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("写入设置失败: {}", e))?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("重命名设置失败: {}", e)
    })
}

/// 读取默认引擎 wire 值；未设置/非法回退 opencode 的 wire（首个内置引擎）。
pub fn default_engine_wire() -> String {
    read_settings()
        .default_engine_id
        .filter(|id| AgentType::from_wire(id).is_some())
        .unwrap_or_else(|| AgentType::OpenCode.as_wire().to_string())
}

/// 写默认引擎（校验为合法 wire 名，否则报错）。
pub fn set_default_engine_wire(wire: &str) -> Result<(), String> {
    if AgentType::from_wire(wire).is_none() {
        return Err(format!("未知引擎: {}", wire));
    }
    let mut settings = read_settings();
    settings.default_engine_id = Some(wire.to_string());
    write_settings(&settings)
}

// ---------- Tauri commands ----------

/// 列出所有内置引擎（含实机可用性）。
#[tauri::command]
pub fn acp_list_agents() -> Vec<AcpAgentInfo> {
    list_agents()
}

/// 读取默认引擎（wire 值）。
#[tauri::command]
pub fn engine_default_get() -> String {
    default_engine_wire()
}

/// 写默认引擎（wire 值；非法报错）。
#[tauri::command]
pub fn engine_default_set(engine_id: String) -> Result<(), String> {
    set_default_engine_wire(&engine_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个内置变体都能取到一致的 agent_type（照 codeg get_agent_meta 测试风格）。
    #[test]
    fn meta_matches_builtin_variants() {
        for &at in BUILTIN_AGENT_TYPES {
            let meta = get_agent_meta(at);
            assert_eq!(meta.agent_type, at);
            assert!(!meta.name.is_empty());
        }
    }

    /// opencode 关键声明字段承载了原硬编码常量。
    #[test]
    fn opencode_meta_carries_launch_constants() {
        let meta = get_agent_meta(AgentType::OpenCode);
        assert_eq!(meta.launch.acp_subcommand, "acp");
        assert!(meta.launch.env_strip.contains(&"OPENCODE_SERVER_USERNAME"));
        assert!(meta.launch.env_strip.contains(&"OPENCODE_SERVER_PASSWORD"));
        assert_eq!(meta.launch.detect_args, &["--version"]);
        assert_eq!(meta.launch.acp_probe_args, &["acp", "--help"]);
        assert_eq!(
            meta.launch.config_path,
            Some(&[".config", "opencode", "opencode.jsonc"][..])
        );
        // 回归：opencode 保持 DirectCommand 形态，无安装指引
        assert_eq!(meta.launch.kind, LaunchKind::DirectCommand);
        assert_eq!(meta.launch.install_hint, None);
    }

    /// Claude/Codex 为 ExternalAdapter 形态，字段与 Step 0 实测一致。
    #[test]
    fn adapter_metas_declare_external_adapter() {
        let claude = get_agent_meta(AgentType::ClaudeCode);
        assert_eq!(claude.launch.kind, LaunchKind::ExternalAdapter);
        assert_eq!(claude.launch.acp_subcommand, "");
        assert_eq!(claude.launch.env_strip, &[] as &[&str]);
        assert_eq!(claude.launch.detect_args, &["--version"]);
        assert_eq!(claude.launch.acp_probe_args, &["--version"]);
        assert_eq!(claude.launch.config_path, None);
        assert_eq!(claude.launch.context_window, ContextWindowSource::None);
        assert_eq!(
            claude.launch.install_hint,
            Some("@agentclientprotocol/claude-agent-acp")
        );
        assert!(
            claude
                .launch
                .bin_candidates
                .iter()
                .any(|c| c.starts_with("claude-agent-acp")),
            "bin_candidates 应含 claude-agent-acp*"
        );

        let codex = get_agent_meta(AgentType::Codex);
        assert_eq!(codex.launch.kind, LaunchKind::ExternalAdapter);
        assert_eq!(codex.launch.acp_subcommand, "");
        assert_eq!(codex.launch.env_strip, &[] as &[&str]);
        assert_eq!(codex.launch.detect_args, &["--version"]);
        assert_eq!(codex.launch.acp_probe_args, &["--version"]);
        assert_eq!(codex.launch.config_path, None);
        assert_eq!(codex.launch.context_window, ContextWindowSource::None);
        assert_eq!(
            codex.launch.install_hint,
            Some("@agentclientprotocol/codex-acp")
        );
        assert!(
            codex
                .launch
                .bin_candidates
                .iter()
                .any(|c| c.starts_with("codex-acp")),
            "bin_candidates 应含 codex-acp*"
        );
    }

    /// npm 全局 bin 目录形态：Windows 即前缀，类 Unix 加 /bin。
    #[test]
    fn npm_bin_dir_from_prefix_shape() {
        assert_eq!(
            npm_bin_dir_from_prefix("C:\\npm-global", true),
            PathBuf::from("C:\\npm-global")
        );
        assert_eq!(
            npm_bin_dir_from_prefix("/usr/local", false),
            PathBuf::from("/usr/local").join("bin")
        );
    }

    /// candidate_in_dir 按候选顺序找真身；无命中返回 None。
    #[test]
    fn candidate_in_dir_matches_in_order() {
        let dir = std::env::temp_dir().join("lightr-vibe-engine-test");
        let _ = std::fs::create_dir_all(&dir);
        let exe = dir.join("codex-acp.cmd");
        let _ = std::fs::write(&exe, "@echo off\r\n");

        let found = candidate_in_dir(&dir, &["codex-acp.cmd", "codex-acp.exe"]);
        assert_eq!(found, Some(exe.to_string_lossy().into_owned()));
        assert_eq!(candidate_in_dir(&dir, &["nope-acp.exe"]), None);

        let _ = std::fs::remove_file(&exe);
    }

    /// list_agents 覆盖所有内置变体。
    #[test]
    fn list_agents_covers_builtins() {
        let agents = list_agents();
        assert_eq!(agents.len(), BUILTIN_AGENT_TYPES.len());
        assert_eq!(agents[0].agent_type.as_wire(), "open_code");
        assert!(agents.iter().any(|a| a.agent_type.as_wire() == "claude_code"));
        assert!(agents.iter().any(|a| a.agent_type.as_wire() == "codex"));
    }

    /// 裸名去扩展（cmd /C 回退用）。
    #[test]
    fn bare_bin_name_strips_executable_ext() {
        let meta = get_agent_meta(AgentType::OpenCode);
        assert_eq!(bare_bin_name(&meta), "opencode");
    }

    /// shim 提取（回归 acp.rs 既有测试：version-fox 形态）。
    #[test]
    fn shim_exe_extraction_version_fox() {
        let shim = "@echo off\r\n\"%dp0%\\node_modules\\opencode-ai\\bin\\opencode.exe\" %*\r\n";
        assert_eq!(
            extract_exe_from_shim(shim, "C:\\Users\\u\\.version-fox\\sdks\\nodejs\\"),
            Some(
                "C:\\Users\\u\\.version-fox\\sdks\\nodejs\\node_modules\\opencode-ai\\bin\\opencode.exe"
                    .to_string()
            )
        );
        assert_eq!(extract_exe_from_shim("echo hello", "C:\\d"), None);
    }

    /// npm 形态 shim（claude-agent-acp）：真身是 `node <脚本>`，只取 node.exe 会丢脚本。
    /// 必须返回 None（→ cmd /C 回退），否则 spawn 裸 node 导致 initialize 永不响应。
    #[test]
    fn shim_exe_extraction_rejects_bare_node_interpreter() {
        // npm 直接形态：%dp0%\node.exe + 另一参数里的 index.js
        let npm_shim = "@echo off\r\nIF EXIST \"%dp0%\\node.exe\" (\r\n  SET \"_prog=%dp0%\\node.exe\"\r\n) ELSE (\r\n  SET \"_prog=node\"\r\n)\r\n\"%_prog%\" \"%dp0%\\node_modules\\@agentclientprotocol\\claude-agent-acp\\dist\\index.js\" %*\r\n";
        assert_eq!(
            extract_exe_from_shim(npm_shim, "C:\\Users\\u\\AppData\\Roaming\\npm\\"),
            None
        );
        // 裸 node 词元也不提取
        let bare = "\"node\" \"C:\\x\\index.js\" %*\r\n";
        assert_eq!(extract_exe_from_shim(bare, "C:\\d"), None);
    }

    #[test]
    fn node_interpreter_detection() {
        assert!(is_node_interpreter("C:\\a\\node.exe"));
        assert!(is_node_interpreter("node"));
        assert!(is_node_interpreter("C:\\x\\NODE.EXE"));
        assert!(!is_node_interpreter("C:\\a\\opencode.exe"));
        assert!(!is_node_interpreter("C:\\a\\nodejs\\claude-agent-acp.cmd"));
    }
}
