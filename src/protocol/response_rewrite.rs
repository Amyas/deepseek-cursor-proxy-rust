use serde_json::Value;

use crate::error::AppError;
use crate::protocol::model::Message;
use crate::reasoning::keys::conversation_scope;
use crate::reasoning::store::ReasoningStore;

use super::folding::fold_reasoning_into_content;
use super::normalize::normalize_message;

fn prefix_response_content(response_payload: &mut Value, prefix: &str) -> bool {
    let Some(choices) = response_payload
        .get_mut("choices")
        .and_then(Value::as_array_mut)
    else {
        return false;
    };
    for choice in choices {
        let Some(message) = choice.get_mut("message").and_then(Value::as_object_mut) else {
            continue;
        };
        let content = message
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        message.insert(
            "content".to_string(),
            Value::String(format!("{prefix}{content}")),
        );
        return true;
    }
    false
}

pub fn record_response_reasoning(
    response_payload: &Value,
    store: Option<&dyn ReasoningStore>,
    request_messages: &[Message],
    cache_namespace: &str,
    scope: Option<&str>,
    prior_messages: Option<&[Message]>,
) -> Result<usize, AppError> {
    let Some(store) = store else {
        return Ok(0);
    };
    let Some(choices) = response_payload.get("choices").and_then(Value::as_array) else {
        return Ok(0);
    };

    let response_scope = scope
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| conversation_scope(request_messages, cache_namespace));
    let response_prior_messages = prior_messages.unwrap_or(request_messages);

    let mut stored = 0usize;
    for choice in choices {
        let Some(message_value) = choice.get("message") else {
            continue;
        };
        let message = normalize_message(message_value, true);
        stored += store.store_assistant_message(
            &message,
            &response_scope,
            cache_namespace,
            Some(response_prior_messages),
        )?;
    }
    Ok(stored)
}

pub fn rewrite_response_body(
    body: &[u8],
    original_model: &str,
    store: Option<&dyn ReasoningStore>,
    request_messages: &[Message],
    cache_namespace: &str,
    content_prefix: Option<&str>,
    display_reasoning: bool,
    collapsible_reasoning: bool,
) -> Result<Vec<u8>, AppError> {
    let mut response_payload: Value =
        serde_json::from_slice(body).map_err(|error| AppError::Upstream(error.to_string()))?;

    if let Some(prefix) = content_prefix {
        prefix_response_content(&mut response_payload, prefix);
    }

    record_response_reasoning(
        &response_payload,
        store,
        request_messages,
        cache_namespace,
        None,
        None,
    )?;

    if display_reasoning {
        fold_reasoning_into_content(&mut response_payload, collapsible_reasoning);
    }

    if let Some(model) = response_payload.get_mut("model") {
        *model = Value::String(original_model.to_string());
    }

    serde_json::to_vec(&response_payload).map_err(|error| AppError::Upstream(error.to_string()))
}

#[cfg(test)]
mod tests {
    use serde_json::Value;
    use serde_json::json;

    use crate::protocol::model::Message;
    use crate::reasoning::keys::{conversation_scope, message_signature};
    use crate::reasoning::sqlite_store::SqliteReasoningStore;
    use crate::reasoning::store::ReasoningStore;

    use super::rewrite_response_body;

    #[test]
    fn records_reasoning_and_restores_original_model_name() {
        let store = SqliteReasoningStore::new(":memory:", None, None).unwrap();
        let body = serde_json::to_vec(&json!({
            "id": "chatcmpl",
            "object": "chat.completion",
            "model": "deepseek-v4-pro",
            "choices": [{
                "index": 0,
                "finish_reason": "stop",
                "message": {
                    "role": "assistant",
                    "content": "Final.",
                    "reasoning_content": "Done thinking."
                }
            }]
        }))
        .unwrap();
        let request_messages = vec![Message {
            role: "user".to_string(),
            content: Some(json!("hi")),
            ..Message::default()
        }];

        let rewritten = rewrite_response_body(
            &body,
            "deepseek-v4-pro",
            Some(&store),
            &request_messages,
            "",
            None,
            false,
            true,
        )
        .unwrap();

        let payload: Value = serde_json::from_slice(&rewritten).unwrap();
        assert_eq!(payload["model"], "deepseek-v4-pro");
        let message =
            crate::protocol::normalize::normalize_message(&payload["choices"][0]["message"], true);
        let stored = store
            .get(&format!(
                "scope:{}:signature:{}",
                conversation_scope(&request_messages, ""),
                message_signature(&message)
            ))
            .unwrap();
        assert_eq!(stored, Some("Done thinking.".to_string()));
    }

    #[test]
    fn recovery_notice_is_prefixed_into_response_content() {
        let store = SqliteReasoningStore::new(":memory:", None, None).unwrap();
        let body = serde_json::to_vec(&json!({
            "id": "chatcmpl",
            "object": "chat.completion",
            "model": "deepseek-v4-pro",
            "choices": [{
                "index": 0,
                "finish_reason": "stop",
                "message": {"role": "assistant", "content": "Final."}
            }]
        }))
        .unwrap();
        let rewritten = rewrite_response_body(
            &body,
            "deepseek-v4-pro",
            Some(&store),
            &[Message {
                role: "user".to_string(),
                content: Some(json!("hi")),
                ..Message::default()
            }],
            "",
            Some("[deepseek-cursor-proxy] Refreshed reasoning_content history.\n\n"),
            false,
            true,
        )
        .unwrap();
        let payload: Value = serde_json::from_slice(&rewritten).unwrap();
        assert!(
            payload["choices"][0]["message"]["content"]
                .as_str()
                .unwrap()
                .starts_with("[deepseek-cursor-proxy] Refreshed reasoning_content history.")
        );
    }

    #[test]
    fn preserves_usage_fields_and_can_display_reasoning() {
        let store = SqliteReasoningStore::new(":memory:", None, None).unwrap();
        let body = serde_json::to_vec(&json!({
            "id": "chatcmpl",
            "object": "chat.completion",
            "model": "deepseek-v4-pro",
            "choices": [{
                "index": 0,
                "finish_reason": "stop",
                "message": {
                    "role": "assistant",
                    "content": "ok",
                    "reasoning_content": "plan"
                }
            }],
            "usage": {
                "prompt_tokens": 10,
                "prompt_cache_hit_tokens": 6,
                "prompt_cache_miss_tokens": 4,
                "completion_tokens": 1,
                "total_tokens": 11
            }
        }))
        .unwrap();
        let rewritten = rewrite_response_body(
            &body,
            "deepseek-v4-flash",
            Some(&store),
            &[],
            "",
            None,
            true,
            true,
        )
        .unwrap();
        let payload: Value = serde_json::from_slice(&rewritten).unwrap();
        assert_eq!(payload["usage"]["prompt_cache_hit_tokens"], 6);
        assert_eq!(payload["usage"]["prompt_cache_miss_tokens"], 4);
        assert!(
            payload["choices"][0]["message"]["content"]
                .as_str()
                .unwrap()
                .contains("Thinking")
        );
    }
}
