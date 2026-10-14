// 引擎身份层：编译期枚举 AgentType + wire 持久化契约（照 codeg `models/agent.rs` 裁剪）。
// wire 名写入 sessions-index.json 的 engineId，**一旦落盘不可变**（改名会孤立已存会话），
// 由 `builtin_wire_names_are_unchanged` 单测锁死。
// 本期内置 `OpenCode` / `ClaudeCode` / `Codex` 三个变体；不含 codeg 的 `Custom`
// 变体（自定义引擎 Out of Scope）。

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// 会话/连接背后的引擎。新增引擎 = 加一个变体（编译器会强制补全所有 match 臂）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentType {
    OpenCode,
    /// Anthropic Claude Code（经官方 ACP 适配器 `claude-agent-acp` 桥接，用户自装）。
    ClaudeCode,
    /// OpenAI Codex（经官方 ACP 适配器 `codex-acp` 桥接，用户自装）。
    Codex,
}

/// 所有内置变体（供单测遍历穷尽性）。
pub const BUILTIN_AGENT_TYPES: &[AgentType] =
    &[AgentType::OpenCode, AgentType::ClaudeCode, AgentType::Codex];

impl AgentType {
    /// 稳定的 wire/持久化表示（写入 sessions-index.json 的 engineId）。
    /// 内置 wire 名是持久化契约，不可变（`claude_code` / `codex` 照 codeg
    /// `models/agent.rs:95,96` 的既有字符串，保证语义一致、将来数据互通无歧义）。
    pub fn as_wire(&self) -> &'static str {
        match self {
            AgentType::OpenCode => "open_code",
            AgentType::ClaudeCode => "claude_code",
            AgentType::Codex => "codex",
        }
    }

    /// 解析 as_wire 的输出。未知 wire → None（上层报明确错误）。
    pub fn from_wire(s: &str) -> Option<Self> {
        match s {
            "open_code" => Some(AgentType::OpenCode),
            "claude_code" => Some(AgentType::ClaudeCode),
            "codex" => Some(AgentType::Codex),
            _ => None,
        }
    }
}

impl fmt::Display for AgentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_wire())
    }
}

impl Serialize for AgentType {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_wire())
    }
}

impl<'de> Deserialize<'de> for AgentType {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        AgentType::from_wire(&s).ok_or_else(|| {
            serde::de::Error::custom(format!("unknown agent type wire name: {}", s))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// wire 名不可变（照 codeg `builtin_wire_names_are_unchanged`）：
    /// 改名会静默孤立已持久化的 sessions-index.json 行。
    #[test]
    fn builtin_wire_names_are_unchanged() {
        assert_eq!(AgentType::OpenCode.as_wire(), "open_code");
        assert_eq!(AgentType::ClaudeCode.as_wire(), "claude_code");
        assert_eq!(AgentType::Codex.as_wire(), "codex");
    }

    /// as_wire ↔ from_wire 往返；BUILTIN 覆盖所有变体。
    #[test]
    fn wire_roundtrip_for_all_builtins() {
        for at in BUILTIN_AGENT_TYPES {
            assert_eq!(AgentType::from_wire(at.as_wire()), Some(*at));
        }
    }

    /// BUILTIN 枚举全部三变体（新增变体忘补 BUILTIN 时此测失败）。
    #[test]
    fn builtin_covers_all_variants() {
        assert_eq!(BUILTIN_AGENT_TYPES.len(), 3);
        assert!(BUILTIN_AGENT_TYPES.contains(&AgentType::OpenCode));
        assert!(BUILTIN_AGENT_TYPES.contains(&AgentType::ClaudeCode));
        assert!(BUILTIN_AGENT_TYPES.contains(&AgentType::Codex));
    }

    /// 未知 wire 名拒绝解析（不静默回退）。
    #[test]
    fn unknown_wire_is_rejected() {
        assert_eq!(AgentType::from_wire("claude"), None);
        assert_eq!(AgentType::from_wire("claude_code_x"), None);
        assert_eq!(AgentType::from_wire("codex_acp"), None);
        assert_eq!(AgentType::from_wire(""), None);
        assert_eq!(AgentType::from_wire("OpenCode"), None);
    }

    /// serde 走 wire：序列化为 wire 名，未知报错。
    #[test]
    fn serde_uses_wire_contract() {
        assert_eq!(serde_json::to_string(&AgentType::OpenCode).unwrap(), "\"open_code\"");
        assert_eq!(
            serde_json::from_str::<AgentType>("\"open_code\"").unwrap(),
            AgentType::OpenCode
        );
        assert_eq!(
            serde_json::to_string(&AgentType::ClaudeCode).unwrap(),
            "\"claude_code\""
        );
        assert_eq!(
            serde_json::to_string(&AgentType::Codex).unwrap(),
            "\"codex\""
        );
        assert!(serde_json::from_str::<AgentType>("\"bogus\"").is_err());
    }
}
