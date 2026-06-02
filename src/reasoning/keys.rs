use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::protocol::model::{Message, ToolCall};

fn sha256_hex(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn canonicalize_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let ordered: BTreeMap<String, Value> = map
                .iter()
                .map(|(key, value)| (key.clone(), canonicalize_value(value)))
                .collect();
            Value::Object(ordered.into_iter().collect())
        }
        Value::Array(items) => Value::Array(items.iter().map(canonicalize_value).collect()),
        _ => value.clone(),
    }
}

fn canonical_json_string(value: &Value) -> String {
    serde_json::to_string(&canonicalize_value(value)).expect("canonical JSON")
}

pub fn normalize_tool_call(tool_call: &ToolCall) -> Value {
    let mut object = Map::new();
    if let Some(id) = &tool_call.id {
        object.insert("id".to_string(), Value::String(id.clone()));
    }
    object.insert(
        "type".to_string(),
        Value::String(tool_call.tool_type.clone()),
    );
    object.insert(
        "function".to_string(),
        json!({
            "name": tool_call.function.name,
            "arguments": tool_call.function.arguments,
        }),
    );
    Value::Object(object)
}

pub fn tool_call_signature(tool_call: &ToolCall) -> String {
    let mut normalized = normalize_tool_call(tool_call);
    if let Some(object) = normalized.as_object_mut() {
        object.remove("id");
    }
    sha256_hex(&canonical_json_string(&normalized))
}

pub fn tool_call_ids(message: &Message) -> Vec<String> {
    message
        .tool_calls
        .as_ref()
        .into_iter()
        .flat_map(|tool_calls| tool_calls.iter())
        .filter_map(|tool_call| tool_call.id.clone())
        .collect()
}

pub fn tool_call_names(message: &Message) -> Vec<String> {
    message
        .tool_calls
        .as_ref()
        .into_iter()
        .flat_map(|tool_calls| tool_calls.iter())
        .map(|tool_call| tool_call.function.name.clone())
        .filter(|name| !name.is_empty())
        .collect()
}

pub fn message_signature(message: &Message) -> String {
    let tool_calls = message
        .tool_calls
        .as_ref()
        .map(|tool_calls| {
            tool_calls
                .iter()
                .map(normalize_tool_call)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let payload = json!({
        "content": message.content.clone().unwrap_or(Value::String(String::new())),
        "tool_calls": tool_calls,
    });
    sha256_hex(&canonical_json_string(&payload))
}

pub fn canonical_scope_message(message: &Message) -> Value {
    let mut object = Map::new();
    object.insert("role".to_string(), Value::String(message.role.clone()));

    if let Some(content) = &message.content {
        object.insert("content".to_string(), content.clone());
    }
    if let Some(name) = &message.name {
        object.insert("name".to_string(), Value::String(name.clone()));
    }
    if let Some(tool_call_id) = &message.tool_call_id {
        object.insert(
            "tool_call_id".to_string(),
            Value::String(tool_call_id.clone()),
        );
    }
    if let Some(prefix) = message.prefix {
        object.insert("prefix".to_string(), Value::Bool(prefix));
    }
    if let Some(tool_calls) = &message.tool_calls {
        object.insert(
            "tool_calls".to_string(),
            Value::Array(tool_calls.iter().map(normalize_tool_call).collect()),
        );
    }

    Value::Object(object)
}

pub fn conversation_scope(messages: &[Message], namespace: &str) -> String {
    let scope_messages = Value::Array(messages.iter().map(canonical_scope_message).collect());
    let payload = if namespace.is_empty() {
        scope_messages
    } else {
        json!({
            "namespace": namespace,
            "messages": scope_messages,
        })
    };
    sha256_hex(&canonical_json_string(&payload))
}

pub fn turn_context_signature(prior_messages: &[Message]) -> String {
    let last_user_index = prior_messages
        .iter()
        .rposition(|message| message.role == "user");

    let mut start_index = 0usize;
    if let Some(index) = last_user_index {
        start_index = index;
        while start_index > 0 && prior_messages[start_index - 1].role == "user" {
            start_index -= 1;
        }
    }

    let context_messages = Value::Array(
        prior_messages[start_index..]
            .iter()
            .filter(|message| message.role != "system")
            .map(canonical_scope_message)
            .collect(),
    );
    sha256_hex(&canonical_json_string(&context_messages))
}

pub fn scoped_reasoning_keys(message: &Message, scope: &str) -> Vec<String> {
    let mut keys = vec![format!(
        "scope:{scope}:signature:{}",
        message_signature(message)
    )];
    keys.extend(
        tool_call_ids(message)
            .into_iter()
            .map(|tool_call_id| format!("scope:{scope}:tool_call:{tool_call_id}")),
    );
    keys.extend(
        message
            .tool_calls
            .as_ref()
            .into_iter()
            .flat_map(|tool_calls| tool_calls.iter())
            .map(|tool_call| {
                format!(
                    "scope:{scope}:tool_call_signature:{}",
                    tool_call_signature(tool_call)
                )
            }),
    );
    keys.extend(
        tool_call_names(message)
            .into_iter()
            .map(|tool_name| format!("scope:{scope}:tool_name:{tool_name}")),
    );
    keys
}

pub fn portable_reasoning_keys(
    message: &Message,
    cache_namespace: &str,
    prior_messages: &[Message],
) -> Vec<String> {
    if cache_namespace.is_empty() {
        return Vec::new();
    }

    let turn_signature = turn_context_signature(prior_messages);
    let mut keys = vec![format!(
        "namespace:{cache_namespace}:turn:{turn_signature}:signature:{}",
        message_signature(message)
    )];
    keys.extend(tool_call_ids(message).into_iter().map(|tool_call_id| {
        format!("namespace:{cache_namespace}:turn:{turn_signature}:tool_call:{tool_call_id}")
    }));
    keys.extend(
        message
            .tool_calls
            .as_ref()
            .into_iter()
            .flat_map(|tool_calls| tool_calls.iter())
            .map(|tool_call| {
                format!(
                    "namespace:{cache_namespace}:turn:{turn_signature}:tool_call_signature:{}",
                    tool_call_signature(tool_call)
                )
            }),
    );
    keys.extend(tool_call_names(message).into_iter().map(|tool_name| {
        format!("namespace:{cache_namespace}:turn:{turn_signature}:tool_name:{tool_name}")
    }));
    keys
}

#[cfg(test)]
mod tests {
    use super::{
        conversation_scope, message_signature, portable_reasoning_keys, tool_call_ids,
        tool_call_names, tool_call_signature, turn_context_signature,
    };
    use crate::protocol::model::{Message, ToolCall, ToolFunction};
    use serde_json::json;

    pub fn scope_prefix(scope: &str) -> String {
        format!("scope:{scope}")
    }

    fn sample_tool_call() -> ToolCall {
        ToolCall {
            id: Some("call_1".to_string()),
            tool_type: "function".to_string(),
            function: ToolFunction {
                name: "lookup".to_string(),
                arguments: "{}".to_string(),
            },
        }
    }

    fn sample_message() -> Message {
        Message {
            role: "assistant".to_string(),
            content: Some(json!("answer")),
            tool_calls: Some(vec![sample_tool_call()]),
            ..Message::default()
        }
    }

    #[test]
    fn builds_scope_prefix() {
        assert_eq!(scope_prefix("abc"), "scope:abc");
    }

    #[test]
    fn produces_signatures_and_keys() {
        let message = sample_message();
        assert_eq!(tool_call_ids(&message), vec!["call_1".to_string()]);
        assert_eq!(tool_call_names(&message), vec!["lookup".to_string()]);
        assert_eq!(message_signature(&message).len(), 64);
        assert_eq!(tool_call_signature(&sample_tool_call()).len(), 64);
    }

    #[test]
    fn scope_changes_with_namespace() {
        let message = sample_message();
        let default_scope = conversation_scope(std::slice::from_ref(&message), "");
        let namespaced_scope = conversation_scope(std::slice::from_ref(&message), "ns");
        assert_ne!(default_scope, namespaced_scope);
    }

    #[test]
    fn turn_context_uses_last_user_cluster() {
        let messages = vec![
            Message {
                role: "system".to_string(),
                content: Some(json!("system")),
                ..Message::default()
            },
            Message {
                role: "user".to_string(),
                content: Some(json!("a")),
                ..Message::default()
            },
            Message {
                role: "assistant".to_string(),
                content: Some(json!("b")),
                ..Message::default()
            },
            Message {
                role: "user".to_string(),
                content: Some(json!("c")),
                ..Message::default()
            },
        ];
        assert_eq!(turn_context_signature(&messages).len(), 64);
        let portable = portable_reasoning_keys(&sample_message(), "ns", &messages);
        assert!(!portable.is_empty());
    }
}
