use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};

use crate::app::AppState;

use super::handlers::{chat_completions, healthz, models, options_handler};

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz).options(options_handler))
        .route("/v1/healthz", get(healthz).options(options_handler))
        .route("/models", get(models).options(options_handler))
        .route("/v1/models", get(models).options(options_handler))
        .route(
            "/chat/completions",
            post(chat_completions).options(options_handler),
        )
        .route(
            "/v1/chat/completions",
            post(chat_completions).options(options_handler),
        )
        .layer(DefaultBodyLimit::max(state.config.max_request_body_bytes))
        .with_state(state)
}

pub fn registered_route_count() -> usize {
    6
}
