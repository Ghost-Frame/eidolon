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

use crate::AppState;
use crate::UserIdentity;
use crate::audit::AuditRecord;
use crate::routes::{activity, audit as audit_route, brain, gate, growth, prompt, sessions, tasks};
use crate::proxy::handler::{proxy_messages, proxy_stats};

async fn health() -> Json<serde_json::Value> {
    Json(json!({
        "ok": true,
        "service": "eidolon-daemon",
        "version": "0.1.0",
    }))
}

async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    mut req: Request,
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
            // Inject system identity for gate bypass
            req.extensions_mut().insert(UserIdentity("system".to_string()));
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

    // Match against all configured API keys using timing-safe comparison
    let matched_user = state.config.auth.api_keys.iter().find_map(|entry| {
        if constant_time_eq(provided_key.as_bytes(), entry.key.as_bytes()) {
            Some(entry.user.clone())
        } else {
            None
        }
    });

    match matched_user {
        Some(user) => {
            req.extensions_mut().insert(UserIdentity(user));
            Ok(next.run(req).await)
        }
        None => Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "unauthorized"})),
        )),
    }
}

/// Constant-time byte comparison using SHA-256 digests.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let ha = Sha256::digest(a);
    let hb = Sha256::digest(b);
    let mut result: u8 = 0;
    for (x, y) in ha.iter().zip(hb.iter()) {
        result |= x ^ y;
    }
    result |= (a.len() != b.len()) as u8;
    std::hint::black_box(result) == 0
}

async fn rate_limit_middleware(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    // Skip if rate limiting is disabled
    let limiter = match &state.rate_limiter {
        Some(l) => l,
        None => return Ok(next.run(req).await),
    };

    // Skip for health checks
    if req.uri().path() == "/health" {
        return Ok(next.run(req).await);
    }

    // Skip for system user (gate bypass from localhost)
    let user = req.extensions().get::<UserIdentity>().map(|u| u.0.clone());
    if user.as_deref() == Some("system") {
        return Ok(next.run(req).await);
    }

    let user_key = user.unwrap_or_else(|| "anonymous".to_string());

    match limiter.check(&user_key) {
        Ok(info) => {
            let mut response = next.run(req).await;
            let headers = response.headers_mut();
            headers.insert("X-RateLimit-Limit", info.limit.into());
            headers.insert("X-RateLimit-Remaining", info.remaining.into());
            headers.insert("X-RateLimit-Reset", info.reset_secs.into());
            Ok(response)
        }
        Err(exceeded) => {
            Err((
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({
                    "error": "rate limit exceeded",
                    "retry_after_secs": exceeded.retry_after_secs,
                    "limit": exceeded.limit,
                })),
            ))
        }
    }
}

async fn audit_middleware(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    // Skip if audit is disabled
    let audit_log = match &state.audit_log {
        Some(a) => a.clone(),
        None => return next.run(req).await,
    };

    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let source_ip = req
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|ci| ci.0.to_string())
        .unwrap_or_default();
    let user_agent = req
        .headers()
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let user = req
        .extensions()
        .get::<UserIdentity>()
        .map(|u| u.0.clone())
        .unwrap_or_else(|| "anonymous".to_string());

    let response = next.run(req).await;

    let status_code = response.status().as_u16();
    let timestamp = chrono::Utc::now().to_rfc3339();

    audit_log.record(AuditRecord {
        timestamp,
        user,
        method,
        path,
        status_code,
        source_ip,
        user_agent,
    });

    response
}

pub fn build_router(state: Arc<AppState>) -> Router {
    let cors = if state.config.safety.cors_origins.is_empty() {
        let origin = format!("http://{}:{}", state.config.server.host, state.config.server.port);
        CorsLayer::new()
            .allow_origin(origin.parse::<axum::http::HeaderValue>().unwrap())
            .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
            .allow_headers(Any)
    } else {
        let origins: Vec<axum::http::HeaderValue> = state.config.safety.cors_origins.iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
            .allow_headers(Any)
    };

    // Main routes with auth + rate limiting + audit
    let main_routes = Router::new()
        .route("/health", get(health))
        .route("/activity", post(activity::post_activity))
        .route("/task", post(tasks::submit_task))
        .route("/task/{id}", get(tasks::task_status))
        .route("/task/{id}/kill", post(tasks::kill_task))
        .route("/task/{id}/stream", get(sessions::stream_session))
        .route("/sessions", get(sessions::list_sessions))
        .route("/brain/stats", get(brain::brain_stats))
        .route("/brain/query", post(brain::brain_query))
        .route("/brain/dream", post(brain::brain_dream))
        .route("/gate/check", post(gate::gate_check))
        .route("/gate/complete", post(gate::gate_complete))
        .route("/gate/respond", post(gate::gate_respond))
        .route("/growth/reflect", post(growth::growth_reflect))
        .route("/growth/observations", get(growth::growth_observations))
        .route("/growth/materialize", get(growth::growth_materialize))
        .route("/prompt/generate", post(prompt::generate_prompt))
        .route("/audit", get(audit_route::get_audit_log))
        .layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            audit_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            rate_limit_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            auth_middleware,
        ));

    // Proxy routes -- NO auth middleware (they forward Anthropic API keys)
    let proxy_enabled = state.config.proxy.enabled && state.proxy_state.is_some();
    let router = if proxy_enabled {
        tracing::info!("proxy routes enabled at /v1/messages");
        let proxy_routes = Router::new()
            .route("/v1/messages", post(proxy_messages))
            .route("/proxy/stats", get(proxy_stats));
        main_routes.merge(proxy_routes)
    } else {
        main_routes
    };

    router
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
