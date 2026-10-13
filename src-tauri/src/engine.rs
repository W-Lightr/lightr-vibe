// 引擎静态元数据注册表（照 codeg `acp/registry.rs` 裁剪）。
// 新增引擎 = 加一个 `AgentType` 变体 + 一条 `get_agent_meta` match 臂；所有
// spawn/检测/配置路径/context-window 规则都从 `AcpAgentMeta` 数据取出，避免代码分支膨胀。
// 本期仅 opencode 一条，承载原先散落在 acp.rs / agent.rs 的硬编码常量。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::agent_type::{AgentType, BUILTIN_AGENT_TYPES};

/// context-window 的来源策略（各引擎元数据存放位置不同）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextWindowSource {
    /// opencode：用户配置（config.json/opencode.json/opencode.jsonc）→ models.dev 缓存目录
    OpenCodeConfigAndCatalog,
    /// 无已知来源：返回 None（前端保持既有分母，不臆造）。
    /// 本期无引擎使用，为「新增无 context-window 来源的引擎」预留。
    #[allow(dead_code)]
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

/// 引擎专属的启动/检测/配置声明式规则。
#[derive(Debug, Clone, Copy)]
pub struct LaunchSpec {
    /// resolve_bin 候选（含 .exe/.cmd/.bat；Windows 下按此顺序在 PATH 中查找）
    pub bin_candidates: &'static [&'static str],
    /// ACP 子命令（spawn 时追加到 bin 后，如 `opencode acp`）
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
    },
};

/// 静态查表（infallible；枚举穷尽 match）。本期仅一条臂，穷尽性检查的收益在新增引擎时兑现。
pub fn get_agent_meta(agent_type: AgentType) -> AcpAgentMeta {
    match agent_type {
        AgentType::OpenCode => OPENCODE_META,
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

/// 列出所有内置引擎（本期仅 opencode）；available/installed_version 由实机检测填充。
pub fn list_agents() -> Vec<AcpAgentInfo> {
    BUILTIN_AGENT_TYPES
        .iter()
        .map(|&agent_type| {
            let meta = get_agent_meta(agent_type);
            let available = resolve_bin(&meta).is_some();
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

/// 引擎 spawn 解析结果。
pub enum SpawnSpec {
    /// 直接 spawn 真身 exe（probe §0 路线 1：长驻双向 stdio 进程最干净）
    Exe(PathBuf),
    /// 末选回退：`cmd /C <bin> <subcommand>`（tauri-bridge Gotcha：.cmd 无法 CreateProcess）
    CmdFallback,
}

/// spawn 解析链：which → .cmd shim 文本提取真身 exe → cmd /C 末选（全部按 meta）。
pub fn resolve_spawn(meta: &AcpAgentMeta) -> SpawnSpec {
    if let Some(bin) = resolve_bin(meta) {
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
        // 裸名候选（无扩展名，如类 Unix）：直接 spawn 该路径
        if path.is_file() {
            return SpawnSpec::Exe(path.to_path_buf());
        }
    }
    SpawnSpec::CmdFallback
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
    }

    /// list_agents 覆盖所有内置变体。
    #[test]
    fn list_agents_covers_builtins() {
        let agents = list_agents();
        assert_eq!(agents.len(), BUILTIN_AGENT_TYPES.len());
        assert_eq!(agents[0].agent_type.as_wire(), "open_code");
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
}
