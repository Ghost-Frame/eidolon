use axum::{
    extract::{Request, State},
    http::{Method, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::AppState;
use crate::routes::{brain, gate, sessions, tasks};

async fn health() -> Json<serde_json::Value> {
    Json(json!({
        "ok": true,
        "service": "eidolon-daemon",
        "version": "0.1.0",
    }))
}

async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    // Skip auth for health and gate/check (localhost-only, called by hooks)
    if req.uri().path() == "/health" || req.uri().path() == "/gate/check" {
        return Ok(next.run(req).await);
    }

    let auth_header = req.headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let provided_key = if let Some(bearer) = auth_header.strip_prefix("Bearer ") {
        bearer
    } else {
        ""
    };

    // Timing-safe comparison
    if !constant_time_eq(provided_key.as_bytes(), state.config.api_key.as_bytes()) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "unauthorized"})),
        ));
    }

    Ok(next.run(req).await)
}

/// Constant-time byte comparison to prevent timing attacks on API key
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut result: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        result |= x ^ y;
    }
    result == 0
}

pub fn build_router(state: Arc<AppState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(Any);

    Router::new()
        .route("/health", get(health))
        .route("/task", post(tasks::submit_task))
        .route("/task/{id}", get(tasks::task_status))
        .route("/task/{id}/kill", post(tasks::kill_task))
        .route("/task/{id}/stream", get(sessions::stream_session))
        .route("/sessions", get(sessions::list_sessions))
        .route("/brain/stats", get(brain::brain_stats))
        .route("/brain/query", post(brain::brain_query))
        .route("/gate/check", post(gate::gate_check))
        .layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            auth_middleware,
        ))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
