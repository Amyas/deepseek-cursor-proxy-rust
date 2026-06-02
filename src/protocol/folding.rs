use serde_json::Value;

pub const THINKING_SUMMARY: &str = "Thinking";
pub const THINKING_BLOCK_START: &str = "<think>\n";
pub const THINKING_BLOCK_END: &str = "\n</think>\n\n";
pub const COLLAPSIBLE_THINKING_BLOCK_START: &str = "<details>\n<summary>Thinking</summary>\n\n";
pub const COLLAPSIBLE_THINKING_BLOCK_END: &str = "\n</details>\n\n";

pub fn fold_reasoning_into_content(response_payload: &mut Value, collapsible: bool) {
    let Some(choices) = response_payload
        .get_mut("choices")
        .and_then(Value::as_array_mut)
    else {
        return;
    };

    let block_start = if collapsible {
        COLLAPSIBLE_THINKING_BLOCK_START
    } else {
        THINKING_BLOCK_START
    };
    let block_end = if collapsible {
        COLLAPSIBLE_THINKING_BLOCK_END
    } else {
        THINKING_BLOCK_END
    };

    for choice in choices {
        let Some(message) = choice.get_mut("message").and_then(Value::as_object_mut) else {
            continue;
        };
        let Some(reasoning) = message.get("reasoning_content").and_then(Value::as_str) else {
            continue;
        };
        if reasoning.is_empty() {
            continue;
        }
        let existing_content = message
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        message.insert(
            "content".to_string(),
            Value::String(format!(
                "{block_start}{reasoning}{block_end}{existing_content}"
            )),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::fold_reasoning_into_content;
    use serde_json::json;

    #[test]
    fn folds_reasoning_into_content() {
        let mut payload = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Final.",
                    "reasoning_content": "Plan."
                }
            }]
        });
        fold_reasoning_into_content(&mut payload, true);
        let content = payload["choices"][0]["message"]["content"]
            .as_str()
            .unwrap();
        assert!(content.contains("Thinking"));
        assert!(content.ends_with("Final."));
    }
}
