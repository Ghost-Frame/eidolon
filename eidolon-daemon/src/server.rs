use axum::{
    extract::{Request, State},
    http::{Method, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::routes::{brain, gate, sessions, tasks};
use crate::AppState;

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
    // Skip auth for health check unconditionally
    if req.uri().path() == "/health" {
        return Ok(next.run(req).await);
    }

    // /gate/check is called by localhost hook scripts.
    // Only bypass auth when the request originates from loopback.
    if req.uri().path() == "/gate/check" {
        let is_local = req
            .extensions()
            .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
            .map(|ci| ci.0.ip().is_loopback())
            .unwrap_or(false);
        if is_local {
            return Ok(next.run(req).await);
        }
        // Non-local gate requests fall through to normal auth below
    }

    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let provided_key = if let Some(bearer) = auth_header.strip_prefix("Bearer ") {
        bearer
    } else {
        ""
    };

    // Timing-safe comparison via SHA-256 digest equality
    if !constant_time_eq(provided_key.as_bytes(), state.config.api_key.as_bytes()) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "unauthorized"})),
        ));
    }

    Ok(next.run(req).await)
}

/// Constant-time byte comparison using SHA-256 digests.
/// Hashing both inputs to a fixed-length output prevents length-leaking
/// side-channels present in naive early-exit comparisons.
/// black_box fences prevent the compiler from optimizing the XOR loop
/// into an early-exit branch.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let ha = Sha256::digest(a);
    let hb = Sha256::digest(b);
    let mut result: u8 = 0;
    for (x, y) in ha.iter().zip(hb.iter()) {
        result |= x ^ y;
    }
    std::hint::black_box(result) == 0 && a.len() == b.len()
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
        .route("/gate/complete", post(gate::gate_complete))
        .layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            auth_middleware,
        ))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
