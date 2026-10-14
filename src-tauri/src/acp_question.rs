// ACP `elicitation/create`（server→client 请求）的 Questions 型解析与应答构造。
// codex Plan `request_user_input` 与通用 MCP form 走此路径；当前项目此前对该方法一律
// 回 `-32601`，导致提问静默失败（design §2）。本模块只做**纯函数**分类与响应构造，
// 不触碰进程/连接（协议路由在 acp.rs）。
//
// 协议形状（design §1，核对自 codeg `question.rs:1255,1600`）：
//   请求 params.mode.requestedSchema.properties.<fieldId> = { type, title, enum, ... }
//   响应 result = { action: "accept", content: { <fieldId>: <value> } } | { action: "decline" }
// 本期只认 Questions：single-select / multi-select / free-text（一次一问）。
// Approval 型（MCP 工具审批 / message-only confirm）与其余未知形状 → Decline，
// 让 agent 自行判断，不 hang（design §3.1）。
//
// 宽容解析：codex form 字段名随版本可能漂移，识别不了一律 Decline（不 panic、不 hang）。

use serde::Serialize;
use serde_json::{json, Value};

/// 题型：单选 / 多选 / 自由文本（本期支持范围，design §3.1）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum QuestionKind {
    Single,
    Multi,
    Text,
}

/// 单选/多选的候选项：`value` 回传 content，`label` 展示（title 优先，回退 value）
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QuestionOption {
    pub value: String,
    pub label: String,
}

/// 一个问题（本期一次一问，取 form properties 首字段；其余忽略，design §7）
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Question {
    /// = properties 的键：回传 content 的 key（协议契约，不可丢）
    pub id: String,
    /// 展示提示：title → description → params.message
    pub prompt: String,
    pub kind: QuestionKind,
    /// Single/Multi 有；Text 为空数组
    pub options: Vec<QuestionOption>,
}

/// 分类结果：Questions（本期可渲染）或 Decline（不支持 → 直接回拒答）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElicitationPlan {
    Questions(Vec<Question>),
    Decline,
}

/// 从 elicitaiton `params` 提取问题；无法识别 → Decline。
///
/// 规则（design §3.1）：
/// - `params.mode.requestedSchema.properties` 缺失/非 object/空 → Decline。
/// - 逐字段：`type=="array"` → Multi（选项取 `items.enum`/`items.oneOf`）；
///   `type=="string"` 且含 `enum` → Single；`type=="string"` 无 `enum` → Text；
///   其他类型（boolean/number/…) → 整体 Decline（不支持字段让 agent 自行处理）。
/// - 本期一次一问：只取 properties 首字段，其余忽略（注释见 design §7）。
pub fn classify_elicitation(params: &Value) -> ElicitationPlan {
    let message = params
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let properties = params
        .get("mode")
        .and_then(|m| m.get("requestedSchema"))
        .and_then(|s| s.get("properties"))
        .and_then(Value::as_object);

    let Some(properties) = properties else {
        return ElicitationPlan::Decline;
    };
    if properties.is_empty() {
        return ElicitationPlan::Decline;
    }

    // 本期一次一问（design §7）：只取第一个字段；多字段一次多问留后续
    let (field_id, schema) = properties.iter().next().expect("non-empty checked above");

    let field_type = schema.get("type").and_then(Value::as_str).unwrap_or("");
    let title = schema
        .get("title")
        .and_then(Value::as_str)
        .or_else(|| schema.get("description").and_then(Value::as_str))
        .unwrap_or("")
        .to_string();
    let prompt = if title.is_empty() { message } else { title };

    match field_type {
        "array" => {
            let options = extract_options(
                schema
                    .get("items")
                    .and_then(|i| i.get("enum"))
                    .or_else(|| schema.get("items").and_then(|i| i.get("oneOf"))),
                schema.get("items").and_then(|i| i.get("enumNames")),
            );
            if options.is_empty() {
                return ElicitationPlan::Decline;
            }
            ElicitationPlan::Questions(vec![Question {
                id: field_id.clone(),
                prompt,
                kind: QuestionKind::Multi,
                options,
            }])
        }
        "string" => {
            if let Some(enum_values) = schema.get("enum") {
                let options = extract_options(Some(enum_values), schema.get("enumNames"));
                if options.is_empty() {
                    return ElicitationPlan::Decline;
                }
                ElicitationPlan::Questions(vec![Question {
                    id: field_id.clone(),
                    prompt,
                    kind: QuestionKind::Single,
                    options,
                }])
            } else {
                // 无 enum 的 string = 自由文本
                ElicitationPlan::Questions(vec![Question {
                    id: field_id.clone(),
                    prompt,
                    kind: QuestionKind::Text,
                    options: Vec::new(),
                }])
            }
        }
        // boolean / integer / number / URL / 未知 → 不支持（本期），交给 agent 判断
        _ => ElicitationPlan::Decline,
    }
}

/// 从 `enum`（字符串数组）构造候选项；`enum_names` 为可选展示名（同下标）。
/// 非字符串枚举值（如 oneOf 的 `{const,title}` 形态）取 `const`/`value` + `title`。
fn extract_options(enum_values: Option<&Value>, enum_names: Option<&Value>) -> Vec<QuestionOption> {
    let Some(list) = enum_values.and_then(Value::as_array) else {
        return Vec::new();
    };
    let names = enum_names.and_then(Value::as_array);
    let mut out = Vec::with_capacity(list.len());
    for (i, item) in list.iter().enumerate() {
        let (value, fallback_label) = match item {
            Value::String(s) => (s.clone(), s.clone()),
            Value::Object(obj) => {
                let v = obj
                    .get("const")
                    .or_else(|| obj.get("value"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let label = obj
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or(&v)
                    .to_string();
                (v, label)
            }
            other => (other.to_string(), other.to_string()),
        };
        let label = names
            .and_then(|n| n.get(i))
            .and_then(Value::as_str)
            .map(|s| s.to_string())
            .unwrap_or(fallback_label);
        out.push(QuestionOption { value, label });
    }
    out
}

/// 把用户答案映射成 elicitation 响应 result（design §3.1）。
/// - `declined=true` 或无答案 → `{ "action": "decline" }`
/// - single/text 取首值 → `string`；multi 取全部 → `string[]`
/// - 否则 `{ "action": "accept", "content": { <fieldId>: <value> } }`
///
/// `answers` 为 (fieldId, values) 列表；`plan` 提供字段题型（决定 string vs string[]）。
pub fn build_elicitation_result(
    plan: &[Question],
    answers: &[(String, Vec<String>)],
    declined: bool,
) -> Value {
    if declined || answers.is_empty() {
        return json!({ "action": "decline" });
    }
    let mut content = serde_json::Map::new();
    for (field_id, values) in answers {
        if values.is_empty() {
            continue;
        }
        let is_multi = plan
            .iter()
            .any(|q| &q.id == field_id && q.kind == QuestionKind::Multi);
        if is_multi {
            content.insert(field_id.clone(), json!(values));
        } else {
            content.insert(field_id.clone(), json!(values[0]));
        }
    }
    if content.is_empty() {
        return json!({ "action": "decline" });
    }
    json!({ "action": "accept", "content": Value::Object(content) })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// single-select：string + enum → Single，enum→options（value=枚举值，label=enumNames）
    #[test]
    fn classify_single_select() {
        let params = json!({
            "message": "请选择部署环境",
            "mode": {
                "type": "form",
                "requestedSchema": {
                    "type": "object",
                    "properties": {
                        "env": {
                            "type": "string",
                            "title": "部署环境",
                            "enum": ["dev", "prod"],
                            "enumNames": ["开发", "生产"]
                        }
                    }
                }
            }
        });
        let plan = classify_elicitation(&params);
        let ElicitationPlan::Questions(qs) = plan else {
            panic!("应识别为 Questions");
        };
        assert_eq!(qs.len(), 1);
        assert_eq!(qs[0].id, "env");
        assert_eq!(qs[0].prompt, "部署环境");
        assert_eq!(qs[0].kind, QuestionKind::Single);
        assert_eq!(
            qs[0].options,
            vec![
                QuestionOption { value: "dev".into(), label: "开发".into() },
                QuestionOption { value: "prod".into(), label: "生产".into() },
            ]
        );
    }

    /// single 无 enumNames 时 label 回退为枚举值本身
    #[test]
    fn classify_single_select_without_enum_names() {
        let params = json!({
            "mode": { "requestedSchema": { "properties": {
                "choice": { "type": "string", "enum": ["a", "b"] }
            }}}
        });
        let ElicitationPlan::Questions(qs) = classify_elicitation(&params) else {
            panic!("应识别为 Questions");
        };
        assert_eq!(qs[0].kind, QuestionKind::Single);
        assert_eq!(qs[0].options[1].label, "b");
        // 无 title/description → prompt 回退 message（此处缺 message → 空串）
        assert_eq!(qs[0].prompt, "");
    }

    /// multi-select：array + items.enum → Multi
    #[test]
    fn classify_multi_select() {
        let params = json!({
            "message": "选择要启用的功能",
            "mode": { "requestedSchema": { "properties": {
                "features": {
                    "type": "array",
                    "title": "功能",
                    "items": { "enum": ["a", "b", "c"] }
                }
            }}}
        });
        let ElicitationPlan::Questions(qs) = classify_elicitation(&params) else {
            panic!("应识别为 Questions");
        };
        assert_eq!(qs[0].id, "features");
        assert_eq!(qs[0].kind, QuestionKind::Multi);
        assert_eq!(qs[0].options.len(), 3);
        assert_eq!(qs[0].options[2].value, "c");
    }

    /// free-text：string 无 enum → Text（无选项）
    #[test]
    fn classify_free_text() {
        let params = json!({
            "message": "请输入项目名",
            "mode": { "requestedSchema": { "properties": {
                "name": { "type": "string", "title": "项目名" }
            }}}
        });
        let ElicitationPlan::Questions(qs) = classify_elicitation(&params) else {
            panic!("应识别为 Questions");
        };
        assert_eq!(qs[0].id, "name");
        assert_eq!(qs[0].kind, QuestionKind::Text);
        assert!(qs[0].options.is_empty());
    }

    /// 未知 type → Decline（boolean/number 等本期不支持）
    #[test]
    fn classify_unknown_type_declines() {
        for ty in ["boolean", "integer", "number"] {
            let params = json!({
                "mode": { "requestedSchema": { "properties": {
                    "flag": { "type": ty }
                }}}
            });
            assert_eq!(
                classify_elicitation(&params),
                ElicitationPlan::Decline,
                "type={ty} 应 Decline"
            );
        }
    }

    /// 空 properties / 缺 mode / 缺 requestedSchema → Decline
    #[test]
    fn classify_empty_or_missing_declines() {
        assert_eq!(classify_elicitation(&Value::Null), ElicitationPlan::Decline);
        assert_eq!(
            classify_elicitation(&json!({ "message": "hi" })),
            ElicitationPlan::Decline
        );
        assert_eq!(
            classify_elicitation(&json!({
                "mode": { "requestedSchema": { "type": "object", "properties": {} } }
            })),
            ElicitationPlan::Decline
        );
        // array 但 items 无 enum → 无法构造选项 → Decline
        assert_eq!(
            classify_elicitation(&json!({
                "mode": { "requestedSchema": { "properties": {
                    "x": { "type": "array", "items": {} }
                }}}
            })),
            ElicitationPlan::Decline
        );
    }

    /// build：single/text → string；multi → string[]
    #[test]
    fn build_result_single_multi_text() {
        let single = Question {
            id: "env".into(),
            prompt: "部署环境".into(),
            kind: QuestionKind::Single,
            options: vec![QuestionOption { value: "prod".into(), label: "生产".into() }],
        };
        let multi = Question {
            id: "features".into(),
            prompt: "功能".into(),
            kind: QuestionKind::Multi,
            options: Vec::new(),
        };
        let plan = vec![single, multi];

        // single：取首值 → string
        let r = build_elicitation_result(
            &plan,
            &[("env".to_string(), vec!["prod".to_string()])],
            false,
        );
        assert_eq!(r, json!({ "action": "accept", "content": { "env": "prod" } }));

        // multi：全部 → string[]
        let r = build_elicitation_result(
            &plan,
            &[("features".to_string(), vec!["a".to_string(), "b".to_string()])],
            false,
        );
        assert_eq!(
            r,
            json!({ "action": "accept", "content": { "features": ["a", "b"] } })
        );

        // text（不在 plan 内也按非 multi 处理）：string
        let r = build_elicitation_result(
            &plan,
            &[("note".to_string(), vec!["hello".to_string()])],
            false,
        );
        assert_eq!(r, json!({ "action": "accept", "content": { "note": "hello" } }));
    }

    /// build：declined=true 或空答案 → decline
    #[test]
    fn build_result_declines() {
        let plan: Vec<Question> = Vec::new();
        assert_eq!(
            build_elicitation_result(&plan, &[], true),
            json!({ "action": "decline" })
        );
        assert_eq!(
            build_elicitation_result(&plan, &[], false),
            json!({ "action": "decline" })
        );
        // 空 values 视为无答案 → decline
        assert_eq!(
            build_elicitation_result(&plan, &[("x".to_string(), vec![])], false),
            json!({ "action": "decline" })
        );
    }
}
