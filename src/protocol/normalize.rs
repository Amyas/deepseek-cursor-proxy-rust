use regex::Regex;
use serde_json::{Map, Value, json};
use std::sync::OnceLock;

use super::model::{Message, Tool, ToolCall, ToolFunction};

fn cursor_thinking_block_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?is)(?:<(?:think|thinking)\b[^>]*>[\s\S]*?(?:</(?:think|thinking)>|\z)|<details\b[^>]*>\s*<summary\b[^>]*>\s*Thinking\s*</summary>[\s\S]*?(?:</details>|\z))\s*",
        )
        .expect("valid thinking block regex")
    })
}

pub const SUPPORTED_REQUEST_FIELDS: &[&str] = &[
    "model",
    "messages",
    "stream",
    "stream_options",
    "max_tokens",
    "response_format",
    "stop",
    "tools",
    "tool_choice",
    "thinking",
    "reasoning_effort",
    "temperature",
    "top_p",
    "presence_penalty",
    "frequency_penalty",
    "logprobs",
    "top_logprobs",
    "user",
    "seed",
    "n",
    "logit_bias",
];

pub fn normalize_reasoning_effort(value: &str) -> &str {
    match value.trim().to_ascii_lowercase().as_str() {
        "max" | "xhigh" => "max",
        _ => "high",
    }
}

pub fn extract_text_content(content: &Value) -> Option<String> {
    match content {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                match item {
                    Value::String(text) => parts.push(text.clone()),
                    Value::Object(map) => {
                        let item_type = map.get("type").and_then(Value::as_str);
                        let text = map
                            .get("text")
                            .or_else(|| map.get("content"))
                            .and_then(Value::as_str);
                        if matches!(item_type, Some("text" | "input_text")) {
                            if let Some(text) = text {
                                parts.push(text.to_string());
                            }
                        } else if let Some(text) = text {
                            parts.push(text.to_string());
                        } else if let Some(item_type) = item_type {
                            parts.push(format!("[{item_type} omitted by DeepSeek text proxy]"));
                        }
                    }
                    other => parts.push(other.to_string()),
                }
            }
            Some(parts.join("\n"))
        }
        Value::Object(_) => Some(content.to_string()),
        _ => Some(content.to_string()),
    }
}

pub fn strip_cursor_thinking_blocks(content: &str) -> String {
    cursor_thinking_block_re()
        .replace_all(content, "")
        .trim_start_matches(['\r', '\n'])
        .to_string()
}

pub fn normalize_tool_call(value: &Value) -> ToolCall {
    let object = value.as_object();
    let function = object
        .and_then(|map| map.get("function"))
        .and_then(Value::as_object);

    let arguments = function
        .and_then(|map| map.get("arguments"))
        .map(|value| match value {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        })
        .unwrap_or_default();

    ToolCall {
        id: object
            .and_then(|map| map.get("id"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        tool_type: object
            .and_then(|map| map.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("function")
            .to_string(),
        function: ToolFunction {
            name: function
                .and_then(|map| map.get("name"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            arguments,
        },
    }
}

pub fn normalize_tool(value: &Value) -> Tool {
    if let Some(object) = value.as_object() {
        return Tool {
            tool_type: object
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("function")
                .to_string(),
            function: object
                .get("function")
                .cloned()
                .unwrap_or_else(|| json!({"name": "", "description": "", "parameters": {}})),
        };
    }

    Tool {
        tool_type: "function".to_string(),
        function: json!({"name": "", "description": "", "parameters": {}}),
    }
}

pub fn legacy_function_to_tool(value: &Value) -> Tool {
    Tool {
        tool_type: "function".to_string(),
        function: value.clone(),
    }
}

pub fn convert_function_call(value: &Value) -> Option<Value> {
    match value {
        Value::String(text) if matches!(text.as_str(), "auto" | "none" | "required") => {
            Some(Value::String(text.clone()))
        }
        Value::Object(map) => map.get("name").and_then(Value::as_str).map(|name| {
            json!({
                "type": "function",
                "function": { "name": name }
            })
        }),
        _ => None,
    }
}

pub fn normalize_tool_choice(value: &Value) -> Option<Value> {
    match value {
        Value::String(text) if matches!(text.as_str(), "auto" | "none" | "required") => {
            Some(Value::String(text.clone()))
        }
        Value::String(_) => None,
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("function") {
                if let Some(name) = map
                    .get("function")
                    .and_then(Value::as_object)
                    .and_then(|function| function.get("name"))
                    .and_then(Value::as_str)
                {
                    return Some(json!({
                        "type": "function",
                        "function": { "name": name }
                    }));
                }
            }
            Some(Value::Object(map.clone()))
        }
        _ => Some(value.clone()),
    }
}

pub fn normalize_message(message: &Value, keep_reasoning: bool) -> Message {
    let fallback = || Message {
        role: "user".to_string(),
        content: Some(Value::String(message.to_string())),
        ..Message::default()
    };

    let Some(object) = message.as_object() else {
        return fallback();
    };

    let role = object
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("user")
        .to_string();
    let normalized_role = if role == "function" {
        "tool"
    } else {
        role.as_str()
    };

    let normalized_content = match object.get("content") {
        Some(content) => extract_text_content(content).unwrap_or_default(),
        None if matches!(normalized_role, "assistant" | "tool" | "system" | "user") => {
            String::new()
        }
        None => String::new(),
    };

    let normalized_content = if normalized_role == "assistant" {
        strip_cursor_thinking_blocks(&normalized_content)
    } else {
        normalized_content
    };

    let tool_calls = object
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|calls| calls.iter().map(normalize_tool_call).collect::<Vec<_>>());

    let reasoning_content = if keep_reasoning {
        object
            .get("reasoning_content")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    } else {
        None
    };

    Message {
        role: normalized_role.to_string(),
        content: Some(Value::String(normalized_content)),
        name: object
            .get("name")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        tool_call_id: object
            .get("tool_call_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        tool_calls,
        reasoning_content,
        prefix: object.get("prefix").and_then(Value::as_bool),
    }
}

pub fn normalize_messages(messages: &Value, keep_reasoning: bool) -> Vec<Message> {
    messages
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|message| normalize_message(message, keep_reasoning))
                .collect()
        })
        .unwrap_or_default()
}

pub fn supported_request_fields() -> &'static [&'static str] {
    SUPPORTED_REQUEST_FIELDS
}

pub fn filter_supported_fields(payload: &Map<String, Value>) -> Map<String, Value> {
    payload
        .iter()
        .filter(|(key, _)| SUPPORTED_REQUEST_FIELDS.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        convert_function_call, extract_text_content, filter_supported_fields,
        legacy_function_to_tool, normalize_message, normalize_reasoning_effort, normalize_tool,
        normalize_tool_choice, strip_cursor_thinking_blocks,
    };
    use serde_json::json;

    #[test]
    fn normalizes_reasoning_effort_aliases() {
        assert_eq!(normalize_reasoning_effort("low"), "high");
        assert_eq!(normalize_reasoning_effort("medium"), "high");
        assert_eq!(normalize_reasoning_effort("high"), "high");
        assert_eq!(normalize_reasoning_effort("max"), "max");
        assert_eq!(normalize_reasoning_effort("xhigh"), "max");
        assert_eq!(normalize_reasoning_effort("nonsense"), "high");
    }

    #[test]
    fn extract_text_content_flattens_multipart_array() {
        let content = json!([
            {"type": "text", "text": "hello"},
            {"type": "image_url", "image_url": {"url": "data:..."}},
            {"type": "input_text", "text": "world"}
        ]);
        assert_eq!(
            extract_text_content(&content),
            Some("hello\n[image_url omitted by DeepSeek text proxy]\nworld".to_string())
        );
    }

    #[test]
    fn extract_text_content_passes_through_string_and_none() {
        assert_eq!(
            extract_text_content(&json!("plain")),
            Some("plain".to_string())
        );
        assert_eq!(extract_text_content(&serde_json::Value::Null), None);
    }

    #[test]
    fn strips_cursor_thinking_blocks() {
        assert_eq!(
            strip_cursor_thinking_blocks(
                "<details>\n<summary>Thinking</summary>\n\nplan\n</details>\n\nanswer"
            ),
            "answer"
        );
        assert_eq!(
            strip_cursor_thinking_blocks("<think>\nplan\n</think>\n\nanswer"),
            "answer"
        );
        assert_eq!(
            strip_cursor_thinking_blocks("<details><summary>Diff</summary>\nrelevant\n</details>"),
            "<details><summary>Diff</summary>\nrelevant\n</details>"
        );
    }

    #[test]
    fn normalizes_legacy_and_tool_fields() {
        let tool = legacy_function_to_tool(&json!({"name": "lookup"}));
        assert_eq!(tool.tool_type, "function");
        assert_eq!(tool.function["name"], "lookup");

        let normalized_tool = normalize_tool(&json!({"function": {"name": "lookup"}}));
        assert_eq!(normalized_tool.tool_type, "function");
        assert_eq!(normalized_tool.function["name"], "lookup");
    }

    #[test]
    fn normalizes_function_call_and_tool_choice() {
        assert_eq!(convert_function_call(&json!("auto")), Some(json!("auto")));
        assert_eq!(
            convert_function_call(&json!({"name": "lookup"})),
            Some(json!({"type": "function", "function": {"name": "lookup"}}))
        );
        assert_eq!(
            normalize_tool_choice(&json!({"type": "function", "function": {"name": "lookup"}})),
            Some(json!({"type": "function", "function": {"name": "lookup"}}))
        );
    }

    #[test]
    fn normalizes_message_and_strips_reasoning_when_disabled() {
        let message = normalize_message(
            &json!({
                "role": "assistant",
                "content": "<think>\nplan\n</think>\n\nanswer",
                "reasoning_content": "secret"
            }),
            false,
        );
        assert_eq!(message.role, "assistant");
        assert_eq!(message.content, Some(json!("answer")));
        assert_eq!(message.reasoning_content, None);
    }

    #[test]
    fn filters_supported_fields_only() {
        let payload = json!({
            "model": "deepseek-v4-pro",
            "messages": [],
            "parallel_tool_calls": true
        });
        let map = payload.as_object().unwrap().clone();
        let filtered = filter_supported_fields(&map);
        assert!(filtered.contains_key("model"));
        assert!(filtered.contains_key("messages"));
        assert!(!filtered.contains_key("parallel_tool_calls"));
    }
}
