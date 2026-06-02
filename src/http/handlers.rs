use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Json, response::Result as AxumResult};
use futures_util::StreamExt;
use serde_json::{Value, json};

use crate::app::AppState;
use crate::http::sse::{CursorReasoningDisplayAdapter, StreamAccumulator, rewrite_sse_line};
use crate::protocol::model::Message;
use crate::protocol::normalize::normalize_messages;
use crate::protocol::response_rewrite::rewrite_response_body;
use crate::protocol::transform::prepare_upstream_request;
use crate::trace::model::TraceSummary;

use super::upstream::chat_completions_url;

fn json_error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error": {"message": message}}))).into_response()
}

pub async fn options_handler() -> impl IntoResponse {
    StatusCode::NO_CONTENT
}

pub async fn healthz() -> impl IntoResponse {
    Json(json!({ "ok": true }))
}

pub async fn models(State(state): State<AppState>) -> impl IntoResponse {
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after epoch")
        .as_secs();
    let data = vec![
        json!({
            "id": state.config.upstream_model,
            "object": "model",
            "created": created,
            "owned_by": "deepseek"
        }),
        json!({
            "id": "deepseek-v4-pro",
            "object": "model",
            "created": created,
            "owned_by": "deepseek"
        }),
        json!({
            "id": "deepseek-v4-flash",
            "object": "model",
            "created": created,
            "owned_by": "deepseek"
        }),
    ];
    Json(json!({"object": "list", "data": data}))
}

pub async fn chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> AxumResult<Response> {
    let request_body_for_trace = serde_json::from_slice::<Value>(&body).ok();
    let Some(authorization) = headers.get(axum::http::header::AUTHORIZATION) else {
        return Ok(json_error(
            StatusCode::UNAUTHORIZED,
            "Missing Authorization bearer token",
        ));
    };
    if !authorization
        .to_str()
        .ok()
        .map(|value| value.to_ascii_lowercase().starts_with("bearer "))
        .unwrap_or(false)
    {
        return Ok(json_error(
            StatusCode::UNAUTHORIZED,
            "Missing Authorization bearer token",
        ));
    }

    let payload: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(error) => {
            return Ok(json_error(
                StatusCode::BAD_REQUEST,
                &format!("Invalid JSON: {error}"),
            ));
        }
    };
    let Some(payload_object) = payload.as_object() else {
        return Ok(json_error(
            StatusCode::BAD_REQUEST,
            "Request body must be a JSON object",
        ));
    };

    let prepared = prepare_upstream_request(payload_object, &state.config);
    let request_messages: Vec<Message> = normalize_messages(
        prepared
            .payload
            .get("messages")
            .unwrap_or(&Value::Array(Vec::new())),
        state.config.thinking != "disabled",
    );

    let upstream_url = chat_completions_url(&state.config.upstream_base_url);
    let request_builder = state
        .client
        .post(upstream_url)
        .header(
            axum::http::header::AUTHORIZATION.as_str(),
            HeaderValue::from_bytes(authorization.as_bytes())
                .expect("validated authorization header"),
        )
        .header(
            axum::http::header::CONTENT_TYPE.as_str(),
            "application/json",
        )
        .json(&Value::Object(prepared.payload.clone()));

    let upstream_response = match request_builder.send().await {
        Ok(response) => response,
        Err(error) => {
            return Ok(json_error(
                StatusCode::BAD_GATEWAY,
                &format!("Upstream request failed: {error}"),
            ));
        }
    };

    let status = upstream_response.status();
    let response_headers = upstream_response.headers().clone();
    if prepared
        .payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let mut upstream_stream = upstream_response.bytes_stream();
        let store = state.store.clone();
        let request_messages_for_stream = request_messages.clone();
        let display_reasoning = state.config.display_reasoning;
        let collapsible_reasoning = state.config.collapsible_reasoning;
        let original_model = prepared.original_model.clone();

        let stream = async_stream::stream! {
            let mut accumulator = StreamAccumulator::default();
            let mut display_adapter = display_reasoning.then(|| CursorReasoningDisplayAdapter::new(collapsible_reasoning));
            let mut buffer = Vec::<u8>::new();

            while let Some(item) = upstream_stream.next().await {
                let chunk = match item {
                    Ok(chunk) => chunk,
                    Err(error) => {
                        yield Err::<Bytes, std::io::Error>(std::io::Error::other(error.to_string()));
                        return;
                    }
                };
                buffer.extend_from_slice(&chunk);
                while let Some(position) = buffer.iter().position(|byte| *byte == b'\n') {
                    let line: Vec<u8> = buffer.drain(..=position).collect();
                    let (rewritten, finalized) = match rewrite_sse_line(
                        &line,
                        &original_model,
                        &mut accumulator,
                        "",
                        &request_messages_for_stream,
                        store.as_ref(),
                        display_adapter.as_mut(),
                    ) {
                        Ok(value) => value,
                        Err(error) => {
                            yield Err::<Bytes, std::io::Error>(std::io::Error::other(error.to_string()));
                            return;
                        }
                    };
                    yield Ok::<Bytes, std::io::Error>(Bytes::from(rewritten));
                    if finalized {
                        return;
                    }
                }
            }

            if !buffer.is_empty() {
                let (rewritten, _) = match rewrite_sse_line(
                    &buffer,
                    &original_model,
                    &mut accumulator,
                    "",
                    &request_messages_for_stream,
                    store.as_ref(),
                    display_adapter.as_mut(),
                ) {
                    Ok(value) => value,
                    Err(error) => {
                        yield Err::<Bytes, std::io::Error>(std::io::Error::other(error.to_string()));
                        return;
                    }
                };
                yield Ok::<Bytes, std::io::Error>(Bytes::from(rewritten));
            } else {
                let (rewritten, _) = match rewrite_sse_line(
                    b"data: [DONE]\n\n",
                    &original_model,
                    &mut accumulator,
                    "",
                    &request_messages_for_stream,
                    store.as_ref(),
                    display_adapter.as_mut(),
                ) {
                    Ok(value) => value,
                    Err(error) => {
                        yield Err::<Bytes, std::io::Error>(std::io::Error::other(error.to_string()));
                        return;
                    }
                };
                yield Ok::<Bytes, std::io::Error>(Bytes::from(rewritten));
            }
        };

        let mut response = Response::new(axum::body::Body::from_stream(stream));
        response.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            HeaderValue::from_static("text/event-stream"),
        );
        return Ok(response);
    }

    let response_bytes = match upstream_response.bytes().await {
        Ok(bytes) => bytes,
        Err(error) => {
            return Ok(json_error(
                StatusCode::BAD_GATEWAY,
                &format!("Failed reading upstream response: {error}"),
            ));
        }
    };

    if !status.is_success() {
        let mut response = Response::new(axum::body::Body::from(response_bytes));
        *response.status_mut() =
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        response.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        return Ok(response);
    }

    let rewritten = match rewrite_response_body(
        &response_bytes,
        &prepared.original_model,
        Some(state.store.as_ref()),
        &request_messages,
        "",
        None,
        state.config.display_reasoning,
        state.config.collapsible_reasoning,
    ) {
        Ok(body) => body,
        Err(error) => {
            return Ok(json_error(
                StatusCode::BAD_GATEWAY,
                &format!("Failed rewriting upstream response: {error}"),
            ));
        }
    };

    let mut response = Response::new(axum::body::Body::from(rewritten));
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        response_headers
            .get(axum::http::header::CONTENT_TYPE)
            .cloned()
            .unwrap_or_else(|| HeaderValue::from_static("application/json")),
    );
    if let Some(trace_writer) = &state.trace_writer {
        let _ = trace_writer.write(TraceSummary {
            sequence: 0,
            method: "POST".to_string(),
            path: "/v1/chat/completions".to_string(),
            request_body: request_body_for_trace,
            response_status: Some(200),
        });
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::healthz;

    #[test]
    fn exposes_expected_placeholder_handler_name() {
        let _ = healthz;
    }
}
