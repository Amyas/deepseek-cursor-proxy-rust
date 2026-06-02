use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use deepseek_cursor_proxy_rust::app::AppState;
use deepseek_cursor_proxy_rust::config::AppConfig;
use deepseek_cursor_proxy_rust::http::routes::build_router;
use serde_json::Value;
use tower::util::ServiceExt;

fn temp_db_path() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("after epoch")
        .as_nanos();
    std::env::temp_dir()
        .join("deepseek-cursor-proxy-rust-tests")
        .join(format!("proxy-{unique}.sqlite3"))
}

fn build_test_router() -> axum::Router {
    let state = AppState::new(AppConfig {
        reasoning_content_path: temp_db_path(),
        ..AppConfig::default()
    })
    .unwrap();
    build_router(state)
}

#[tokio::test]
async fn health_and_models_routes_work() {
    let app = build_test_router();

    let health = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);
    let health_body = to_bytes(health.into_body(), usize::MAX).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&health_body).unwrap()["ok"],
        true
    );

    let models = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(models.status(), StatusCode::OK);
    let models_body = to_bytes(models.into_body(), usize::MAX).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&models_body).unwrap()["object"],
        "list"
    );
}

#[tokio::test]
async fn chat_completion_requires_bearer_token() {
    let app = build_test_router();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"model":"deepseek-v4-pro","messages":[]}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn chat_completion_rejects_invalid_json() {
    let app = build_test_router();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header(header::AUTHORIZATION, "Bearer test-key")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
