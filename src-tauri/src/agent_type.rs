// 引擎身份层：编译期枚举 AgentType + wire 持久化契约（照 codeg `models/agent.rs` 裁剪）。
// wire 名写入 sessions-index.json 的 engineId，**一旦落盘不可变**（改名会孤立已存会话），
// 由 `builtin_wire_names_are_unchanged` 单测锁死。
// 本期仅 `OpenCode` 一个内置变体；不含 codeg 的 `Custom` 变体（自定义引擎 Out of Scope）。

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// 会话/连接背后的引擎。新增引擎 = 加一个变体（编译器会强制补全所有 match 臂）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentType {
    OpenCode,
}

/// 所有内置变体（供单测遍历穷尽性；本期仅一项）。
pub const BUILTIN_AGENT_TYPES: &[AgentType] = &[AgentType::OpenCode];

impl AgentType {
    /// 稳定的 wire/持久化表示（写入 sessions-index.json 的 engineId）。
    /// 内置 wire 名是持久化契约，不可变。
    pub fn as_wire(&self) -> &'static str {
        match self {
            AgentType::OpenCode => "open_code",
        }
    }

    /// 解析 as_wire 的输出。未知 wire → None（上层报明确错误）。
    pub fn from_wire(s: &str) -> Option<Self> {
        match s {
            "open_code" => Some(AgentType::OpenCode),
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
    }

    /// as_wire ↔ from_wire 往返；BUILTIN 覆盖所有变体。
    #[test]
    fn wire_roundtrip_for_all_builtins() {
        for at in BUILTIN_AGENT_TYPES {
            assert_eq!(AgentType::from_wire(at.as_wire()), Some(*at));
        }
    }

    /// 未知 wire 名拒绝解析（不静默回退）。
    #[test]
    fn unknown_wire_is_rejected() {
        assert_eq!(AgentType::from_wire("claude_code"), None);
        assert_eq!(AgentType::from_wire(""), None);
        assert_eq!(AgentType::from_wire("OpenCode"), None);
    }

    /// serde 走 wire：序列化为 "open_code"，未知报错。
    #[test]
    fn serde_uses_wire_contract() {
        assert_eq!(serde_json::to_string(&AgentType::OpenCode).unwrap(), "\"open_code\"");
        assert_eq!(
            serde_json::from_str::<AgentType>("\"open_code\"").unwrap(),
            AgentType::OpenCode
        );
        assert!(serde_json::from_str::<AgentType>("\"bogus\"").is_err());
    }
}
