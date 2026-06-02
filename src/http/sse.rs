use serde_json::{Value, json};

use crate::protocol::folding::{
    COLLAPSIBLE_THINKING_BLOCK_END, COLLAPSIBLE_THINKING_BLOCK_START, THINKING_BLOCK_END,
    THINKING_BLOCK_START,
};
use crate::protocol::model::{Message, ToolCall, ToolFunction};
use crate::reasoning::store::ReasoningStore;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StreamingChoice {
    pub role: String,
    pub content: String,
    pub reasoning_content: String,
    pub has_reasoning_content: bool,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: Option<String>,
}

impl StreamingChoice {
    pub fn to_message(&self) -> Message {
        Message {
            role: if self.role.is_empty() {
                "assistant".to_string()
            } else {
                self.role.clone()
            },
            content: Some(Value::String(self.content.clone())),
            reasoning_content: self
                .has_reasoning_content
                .then(|| self.reasoning_content.clone()),
            tool_calls: (!self.tool_calls.is_empty()).then(|| self.tool_calls.clone()),
            ..Message::default()
        }
    }
}

#[derive(Debug, Default)]
pub struct StreamAccumulator {
    pub choices: std::collections::BTreeMap<i64, StreamingChoice>,
    stored_choices: std::collections::BTreeMap<(i64, String), &'static str>,
}

impl StreamAccumulator {
    pub fn ingest_chunk(&mut self, chunk: &Value) {
        let Some(choices) = chunk.get("choices").and_then(Value::as_array) else {
            return;
        };
        for raw_choice in choices {
            let Some(choice_obj) = raw_choice.as_object() else {
                continue;
            };
            let index = choice_obj.get("index").and_then(Value::as_i64).unwrap_or(0);
            let choice = self.choices.entry(index).or_default();
            if let Some(finish_reason) = choice_obj.get("finish_reason").and_then(Value::as_str) {
                choice.finish_reason = Some(finish_reason.to_string());
            }
            let Some(delta) = choice_obj.get("delta").and_then(Value::as_object) else {
                continue;
            };
            if let Some(role) = delta.get("role").and_then(Value::as_str) {
                choice.role = role.to_string();
            }
            if let Some(content) = delta.get("content").and_then(Value::as_str) {
                choice.content.push_str(content);
            }
            if let Some(reasoning_content) = delta.get("reasoning_content").and_then(Value::as_str)
            {
                choice.has_reasoning_content = true;
                choice.reasoning_content.push_str(reasoning_content);
            }
            Self::merge_tool_call_deltas(choice, delta.get("tool_calls"));
        }
    }

    fn merge_tool_call_deltas(choice: &mut StreamingChoice, deltas: Option<&Value>) {
        let Some(deltas) = deltas.and_then(Value::as_array) else {
            return;
        };
        for raw_delta in deltas {
            let Some(delta) = raw_delta.as_object() else {
                continue;
            };
            let index = delta
                .get("index")
                .and_then(Value::as_u64)
                .map(|value| value as usize)
                .unwrap_or(choice.tool_calls.len());
            while choice.tool_calls.len() <= index {
                choice.tool_calls.push(ToolCall {
                    id: None,
                    tool_type: "function".to_string(),
                    function: ToolFunction {
                        name: String::new(),
                        arguments: String::new(),
                    },
                });
            }
            let tool_call = &mut choice.tool_calls[index];
            if let Some(id) = delta.get("id").and_then(Value::as_str) {
                tool_call.id = Some(id.to_string());
            }
            if let Some(tool_type) = delta.get("type").and_then(Value::as_str) {
                tool_call.tool_type = tool_type.to_string();
            }
            let Some(function) = delta.get("function").and_then(Value::as_object) else {
                continue;
            };
            if let Some(name) = function.get("name").and_then(Value::as_str) {
                if tool_call.function.name.is_empty() {
                    tool_call.function.name = name.to_string();
                } else {
                    tool_call.function.name.push_str(name);
                }
            }
            if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                tool_call.function.arguments.push_str(arguments);
            }
        }
    }

    pub fn store_ready_reasoning(
        &mut self,
        store: &dyn ReasoningStore,
        scope: &str,
        cache_namespace: &str,
        prior_messages: &[Message],
    ) -> Result<usize, crate::error::AppError> {
        let mut stored = 0usize;
        let items = self
            .choices
            .iter()
            .map(|(index, choice)| (*index, choice.clone()))
            .collect::<Vec<_>>();
        for (index, choice) in items {
            if choice.finish_reason.is_some() || self.has_identified_tool_calls(&choice) {
                stored += self.store_choice(
                    index,
                    &choice,
                    store,
                    scope,
                    if choice.finish_reason.is_some() {
                        "final"
                    } else {
                        "tool_call"
                    },
                    cache_namespace,
                    prior_messages,
                )?;
            }
        }
        Ok(stored)
    }

    pub fn store_reasoning(
        &mut self,
        store: &dyn ReasoningStore,
        scope: &str,
        cache_namespace: &str,
        prior_messages: &[Message],
    ) -> Result<usize, crate::error::AppError> {
        let mut stored = 0usize;
        let items = self
            .choices
            .iter()
            .map(|(index, choice)| (*index, choice.clone()))
            .collect::<Vec<_>>();
        for (index, choice) in items {
            stored += self.store_choice(
                index,
                &choice,
                store,
                scope,
                "final",
                cache_namespace,
                prior_messages,
            )?;
        }
        Ok(stored)
    }

    fn store_choice(
        &mut self,
        index: i64,
        choice: &StreamingChoice,
        store: &dyn ReasoningStore,
        scope: &str,
        stage: &'static str,
        cache_namespace: &str,
        prior_messages: &[Message],
    ) -> Result<usize, crate::error::AppError> {
        let rank = |value: &'static str| if value == "final" { 2 } else { 1 };
        let storage_key = (index, scope.to_string());
        if let Some(previous_stage) = self.stored_choices.get(&storage_key) {
            if rank(previous_stage) >= rank(stage) {
                return Ok(0);
            }
        }
        let stored = store.store_assistant_message(
            &choice.to_message(),
            scope,
            cache_namespace,
            Some(prior_messages),
        )?;
        if stored > 0 {
            self.stored_choices.insert(storage_key, stage);
        }
        Ok(stored)
    }

    fn has_identified_tool_calls(&self, choice: &StreamingChoice) -> bool {
        choice.has_reasoning_content
            && !choice.tool_calls.is_empty()
            && choice
                .tool_calls
                .iter()
                .all(|tool_call| tool_call.id.is_some())
    }
}

#[derive(Debug)]
pub struct CursorReasoningDisplayAdapter {
    open_choices: std::collections::BTreeSet<i64>,
    last_chunk_metadata: serde_json::Map<String, Value>,
    block_start: &'static str,
    block_end: &'static str,
}

impl CursorReasoningDisplayAdapter {
    pub fn new(collapsible: bool) -> Self {
        Self {
            open_choices: std::collections::BTreeSet::new(),
            last_chunk_metadata: serde_json::Map::new(),
            block_start: if collapsible {
                COLLAPSIBLE_THINKING_BLOCK_START
            } else {
                THINKING_BLOCK_START
            },
            block_end: if collapsible {
                COLLAPSIBLE_THINKING_BLOCK_END
            } else {
                THINKING_BLOCK_END
            },
        }
    }

    pub fn rewrite_chunk(&mut self, chunk: &mut Value) {
        let Some(chunk_obj) = chunk.as_object_mut() else {
            return;
        };
        for key in ["id", "object", "created"] {
            if let Some(value) = chunk_obj.get(key).cloned() {
                self.last_chunk_metadata.insert(key.to_string(), value);
            }
        }
        let Some(choices) = chunk_obj.get_mut("choices").and_then(Value::as_array_mut) else {
            return;
        };
        for raw_choice in choices {
            let Some(choice) = raw_choice.as_object_mut() else {
                continue;
            };
            let index = choice.get("index").and_then(Value::as_i64).unwrap_or(0);
            let finish_reason = choice.get("finish_reason").cloned();
            let delta = choice
                .entry("delta".to_string())
                .or_insert_with(|| Value::Object(Default::default()));
            let Some(delta_obj) = delta.as_object_mut() else {
                continue;
            };

            let mut mirrored_parts = Vec::new();
            if let Some(reasoning_content) =
                delta_obj.get("reasoning_content").and_then(Value::as_str)
            {
                if !reasoning_content.is_empty() {
                    if !self.open_choices.contains(&index) {
                        mirrored_parts.push(self.block_start.to_string());
                        self.open_choices.insert(index);
                    }
                    mirrored_parts.push(reasoning_content.to_string());
                }
            }

            let existing_content = delta_obj.get("content").and_then(Value::as_str);
            let should_close = self.open_choices.contains(&index)
                && (existing_content.is_some()
                    || delta_obj.get("tool_calls").is_some()
                    || finish_reason.as_ref().and_then(Value::as_str).is_some());
            if should_close {
                mirrored_parts.push(self.block_end.to_string());
                self.open_choices.remove(&index);
            }

            if mirrored_parts.is_empty() {
                continue;
            }
            if let Some(existing_content) = existing_content {
                mirrored_parts.push(existing_content.to_string());
            }
            delta_obj.insert(
                "content".to_string(),
                Value::String(mirrored_parts.join("")),
            );
        }
    }

    pub fn flush_chunk(&mut self, model: &str) -> Option<Value> {
        if self.open_choices.is_empty() {
            return None;
        }
        let choices = self
            .open_choices
            .iter()
            .map(|index| {
                json!({
                    "index": index,
                    "delta": { "content": self.block_end },
                    "finish_reason": null
                })
            })
            .collect::<Vec<_>>();
        self.open_choices.clear();

        Some(json!({
            "id": self.last_chunk_metadata.get("id").cloned().unwrap_or_else(|| json!("chatcmpl-reasoning-close")),
            "object": self.last_chunk_metadata.get("object").cloned().unwrap_or_else(|| json!("chat.completion.chunk")),
            "created": self.last_chunk_metadata.get("created").cloned().unwrap_or_else(|| json!(0)),
            "model": model,
            "choices": choices,
        }))
    }
}

pub fn sse_data(chunk: &Value) -> Vec<u8> {
    let mut bytes = b"data: ".to_vec();
    bytes.extend(serde_json::to_vec(chunk).expect("serializable SSE chunk"));
    bytes.extend(b"\n\n");
    bytes
}

pub fn rewrite_sse_line(
    line: &[u8],
    original_model: &str,
    accumulator: &mut StreamAccumulator,
    cache_namespace: &str,
    request_messages: &[Message],
    store: &dyn ReasoningStore,
    display_adapter: Option<&mut CursorReasoningDisplayAdapter>,
) -> Result<(Vec<u8>, bool), crate::error::AppError> {
    fn trim_ascii_whitespace(bytes: &[u8]) -> &[u8] {
        let start = bytes
            .iter()
            .position(|byte| !byte.is_ascii_whitespace())
            .unwrap_or(bytes.len());
        let end = bytes
            .iter()
            .rposition(|byte| !byte.is_ascii_whitespace())
            .map(|index| index + 1)
            .unwrap_or(start);
        &bytes[start..end]
    }

    let stripped = line
        .strip_suffix(b"\r\n" as &[u8])
        .or_else(|| line.strip_suffix(b"\n" as &[u8]))
        .unwrap_or(line);
    if !stripped.starts_with(b"data:") {
        return Ok((line.to_vec(), false));
    }
    let data = trim_ascii_whitespace(&stripped[b"data:".len()..]);
    let scope = crate::reasoning::keys::conversation_scope(request_messages, cache_namespace);

    if data == b"[DONE]" {
        accumulator.store_reasoning(store, &scope, cache_namespace, request_messages)?;
        let mut prefix = Vec::new();
        if let Some(display_adapter) = display_adapter {
            if let Some(closing_chunk) = display_adapter.flush_chunk(original_model) {
                prefix.extend(sse_data(&closing_chunk));
            }
        }
        prefix.extend(b"data: [DONE]\n\n");
        return Ok((prefix, true));
    }

    let mut chunk: Value = match serde_json::from_slice(data) {
        Ok(value) => value,
        Err(_) => return Ok((line.to_vec(), false)),
    };
    accumulator.ingest_chunk(&chunk);
    accumulator.store_ready_reasoning(store, &scope, cache_namespace, request_messages)?;
    if let Some(display_adapter) = display_adapter {
        display_adapter.rewrite_chunk(&mut chunk);
    }
    if let Some(model) = chunk.get_mut("model") {
        *model = Value::String(original_model.to_string());
    }
    Ok((sse_data(&chunk), false))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{CursorReasoningDisplayAdapter, StreamAccumulator, rewrite_sse_line};
    use crate::protocol::model::Message;
    use crate::reasoning::sqlite_store::SqliteReasoningStore;

    #[test]
    fn accumulator_collects_reasoning_and_tool_calls() {
        let mut accumulator = StreamAccumulator::default();
        accumulator.ingest_chunk(&json!({
            "choices": [{
                "index": 0,
                "delta": {
                    "role": "assistant",
                    "reasoning_content": "plan",
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "lookup", "arguments": "{}"}
                    }]
                }
            }]
        }));
        let message = accumulator.choices.get(&0).unwrap().to_message();
        assert_eq!(message.reasoning_content.as_deref(), Some("plan"));
        assert_eq!(message.tool_calls.unwrap()[0].id.as_deref(), Some("call_1"));
    }

    #[test]
    fn display_adapter_mirrors_reasoning_into_content() {
        let mut chunk = json!({
            "id": "x",
            "object": "chat.completion.chunk",
            "created": 1,
            "model": "deepseek-v4-pro",
            "choices": [{
                "index": 0,
                "delta": { "reasoning_content": "plan" },
                "finish_reason": null
            }]
        });
        let mut adapter = CursorReasoningDisplayAdapter::new(true);
        adapter.rewrite_chunk(&mut chunk);
        assert!(
            chunk["choices"][0]["delta"]["content"]
                .as_str()
                .unwrap()
                .contains("Thinking")
        );
    }

    #[test]
    fn rewrite_sse_done_flushes_and_stores() {
        let store = SqliteReasoningStore::new(":memory:", None, None).unwrap();
        let mut accumulator = StreamAccumulator::default();
        let request_messages = vec![Message {
            role: "user".to_string(),
            content: Some(json!("hi")),
            ..Message::default()
        }];
        let mut adapter = CursorReasoningDisplayAdapter::new(true);
        let _ = rewrite_sse_line(
            br#"data: {"id":"x","object":"chat.completion.chunk","created":1,"model":"deepseek-v4-pro","choices":[{"index":0,"delta":{"role":"assistant","reasoning_content":"plan"},"finish_reason":null}]}
"#,
            "deepseek-v4-pro",
            &mut accumulator,
            "",
            &request_messages,
            &store,
            Some(&mut adapter),
        )
        .unwrap();
        let (done, finalized) = rewrite_sse_line(
            b"data: [DONE]\n\n",
            "deepseek-v4-pro",
            &mut accumulator,
            "",
            &request_messages,
            &store,
            Some(&mut adapter),
        )
        .unwrap();
        assert!(finalized);
        assert!(String::from_utf8(done).unwrap().contains("[DONE]"));
    }
}
