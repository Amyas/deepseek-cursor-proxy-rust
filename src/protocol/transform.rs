use serde_json::{Map, Value};

use crate::config::AppConfig;

use super::model::ThinkingConfig;
use super::normalize::{
    convert_function_call, filter_supported_fields, normalize_messages, normalize_reasoning_effort,
    normalize_tool, normalize_tool_choice,
};

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedRequest {
    pub payload: Map<String, Value>,
    pub original_model: String,
    pub upstream_model: String,
    pub patched_reasoning_messages: usize,
    pub missing_reasoning_messages: usize,
}

pub fn prepare_upstream_request(
    payload: &Map<String, Value>,
    config: &AppConfig,
) -> PreparedRequest {
    let original_model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(&config.upstream_model)
        .to_string();
    let upstream_model = if original_model.starts_with("deepseek-") {
        original_model.clone()
    } else {
        config.upstream_model.clone()
    };

    let mut prepared = filter_supported_fields(payload);

    if !prepared.contains_key("max_tokens") {
        if let Some(max_completion_tokens) = payload.get("max_completion_tokens") {
            prepared.insert("max_tokens".to_string(), max_completion_tokens.clone());
        }
    }

    prepared.insert("model".to_string(), Value::String(upstream_model.clone()));

    if prepared
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let mut stream_options = prepared
            .get("stream_options")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        stream_options.insert("include_usage".to_string(), Value::Bool(true));
        prepared.insert("stream_options".to_string(), Value::Object(stream_options));
    }

    if let Some(tools) = prepared.get("tools").and_then(Value::as_array) {
        let normalized = tools
            .iter()
            .map(normalize_tool)
            .map(|tool| serde_json::to_value(tool).expect("serializable tool"))
            .collect::<Vec<_>>();
        prepared.insert("tools".to_string(), Value::Array(normalized));
    } else if let Some(functions) = payload.get("functions").and_then(Value::as_array) {
        let normalized = functions
            .iter()
            .map(super::normalize::legacy_function_to_tool)
            .map(|tool| serde_json::to_value(tool).expect("serializable tool"))
            .collect::<Vec<_>>();
        prepared.insert("tools".to_string(), Value::Array(normalized));
    }

    if let Some(tool_choice) = prepared.get("tool_choice").cloned() {
        match normalize_tool_choice(&tool_choice) {
            Some(value) => {
                prepared.insert("tool_choice".to_string(), value);
            }
            None => {
                prepared.remove("tool_choice");
            }
        }
    } else if let Some(function_call) = payload.get("function_call") {
        if let Some(tool_choice) = convert_function_call(function_call) {
            prepared.insert("tool_choice".to_string(), tool_choice);
        }
    }

    prepared.insert(
        "thinking".to_string(),
        serde_json::to_value(ThinkingConfig {
            kind: config.thinking.clone(),
        })
        .expect("serializable thinking config"),
    );

    if config.thinking == "enabled" {
        prepared.insert(
            "reasoning_effort".to_string(),
            Value::String(normalize_reasoning_effort(&config.reasoning_effort).to_string()),
        );
    }

    let normalized_messages = normalize_messages(
        payload.get("messages").unwrap_or(&Value::Array(Vec::new())),
        config.thinking != "disabled",
    );
    prepared.insert(
        "messages".to_string(),
        serde_json::to_value(normalized_messages).expect("serializable messages"),
    );

    PreparedRequest {
        payload: prepared,
        original_model,
        upstream_model,
        patched_reasoning_messages: 0,
        missing_reasoning_messages: 0,
    }
}

pub const RECOVERY_NOTICE_TEXT: &str =
    "[deepseek-cursor-proxy] Refreshed reasoning_content history.";
pub const RECOVERY_NOTICE_CONTENT: &str =
    "[deepseek-cursor-proxy] Refreshed reasoning_content history.\n\n";

pub fn strip_recovery_notice_for_upstream(messages: &[Value]) -> Vec<Value> {
    messages
        .iter()
        .map(|message| {
            let Some(object) = message.as_object() else {
                return message.clone();
            };
            if object.get("role").and_then(Value::as_str) != Some("assistant") {
                return message.clone();
            }
            let Some(content) = object.get("content").and_then(Value::as_str) else {
                return message.clone();
            };
            if !content.starts_with(RECOVERY_NOTICE_TEXT) {
                return message.clone();
            }

            let mut cleaned = object.clone();
            cleaned.insert(
                "content".to_string(),
                Value::String(
                    content[RECOVERY_NOTICE_TEXT.len()..]
                        .trim_start_matches(['\r', '\n'])
                        .to_string(),
                ),
            );
            Value::Object(cleaned)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::config::AppConfig;

    use super::{
        PreparedRequest, RECOVERY_NOTICE_CONTENT, RECOVERY_NOTICE_TEXT, prepare_upstream_request,
        strip_recovery_notice_for_upstream,
    };

    fn assert_prepared<F>(payload: serde_json::Value, check: F)
    where
        F: FnOnce(PreparedRequest),
    {
        let prepared =
            prepare_upstream_request(payload.as_object().unwrap(), &AppConfig::default());
        check(prepared);
    }

    #[test]
    fn converts_legacy_functions_to_tools() {
        assert_prepared(
            json!({
                "model": "deepseek-v4-pro",
                "messages": [{"role": "user", "content": "hi"}],
                "functions": [{"name": "lookup", "parameters": {"type": "object"}}],
                "function_call": "auto"
            }),
            |prepared| {
                assert_eq!(prepared.payload["tools"][0]["function"]["name"], "lookup");
                assert_eq!(prepared.payload["tool_choice"], "auto");
            },
        );
    }

    #[test]
    fn named_function_call_becomes_named_tool_choice() {
        assert_prepared(
            json!({
                "model": "deepseek-v4-pro",
                "messages": [{"role": "user", "content": "hi"}],
                "function_call": {"name": "lookup"}
            }),
            |prepared| {
                assert_eq!(
                    prepared.payload["tool_choice"],
                    json!({"type": "function", "function": {"name": "lookup"}})
                );
            },
        );
    }

    #[test]
    fn max_completion_tokens_is_aliased_to_max_tokens() {
        assert_prepared(
            json!({
                "model": "deepseek-v4-pro",
                "messages": [{"role": "user", "content": "hi"}],
                "max_completion_tokens": 256
            }),
            |prepared| {
                assert_eq!(prepared.payload["max_tokens"], 256);
            },
        );
    }

    #[test]
    fn standard_openai_fields_are_forwarded() {
        assert_prepared(
            json!({
                "model": "deepseek-v4-pro",
                "messages": [{"role": "user", "content": "hi"}],
                "user": "user-abc",
                "seed": 42,
                "n": 1,
                "logit_bias": {"50256": -100}
            }),
            |prepared| {
                assert_eq!(prepared.payload["user"], "user-abc");
                assert_eq!(prepared.payload["seed"], 42);
                assert_eq!(prepared.payload["n"], 1);
                assert_eq!(prepared.payload["logit_bias"], json!({"50256": -100}));
            },
        );
    }

    #[test]
    fn unknown_fields_are_dropped() {
        assert_prepared(
            json!({
                "model": "deepseek-v4-pro",
                "messages": [{"role": "user", "content": "hi"}],
                "parallel_tool_calls": true,
                "service_tier": "fast"
            }),
            |prepared| {
                assert!(!prepared.payload.contains_key("parallel_tool_calls"));
                assert!(!prepared.payload.contains_key("service_tier"));
            },
        );
    }

    #[test]
    fn non_deepseek_model_is_rewritten() {
        let config = AppConfig::default();
        let payload = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let prepared = prepare_upstream_request(payload.as_object().unwrap(), &config);
        assert_eq!(prepared.payload["model"], "deepseek-v4-pro");
    }

    #[test]
    fn thinking_disabled_strips_reasoning_from_history() {
        let config = AppConfig {
            thinking: "disabled".to_string(),
            ..AppConfig::default()
        };
        let payload = json!({
            "model": "deepseek-v4-pro",
            "messages": [
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": "answer", "reasoning_content": "hidden"}
            ]
        });
        let prepared = prepare_upstream_request(payload.as_object().unwrap(), &config);
        assert_eq!(prepared.payload["thinking"], json!({"type": "disabled"}));
        assert!(
            prepared.payload["messages"][1]
                .get("reasoning_content")
                .is_none()
        );
    }

    #[test]
    fn recovery_notice_stripping_returns_copy() {
        let original = vec![json!({
            "role": "assistant",
            "content": format!("{RECOVERY_NOTICE_CONTENT}answer")
        })];
        let stripped = strip_recovery_notice_for_upstream(&original);
        assert_eq!(
            original[0]["content"],
            format!("{RECOVERY_NOTICE_CONTENT}answer")
        );
        assert_eq!(stripped[0]["content"], "answer");
        assert_ne!(original[0], stripped[0]);
    }

    #[test]
    fn recovery_notice_constants_align() {
        assert!(RECOVERY_NOTICE_CONTENT.starts_with(RECOVERY_NOTICE_TEXT));
    }
}
