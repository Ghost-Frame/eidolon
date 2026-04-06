use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures::StreamExt;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio_stream::wrappers::ReceiverStream;

use super::capture::run_capture;
use super::recall::run_recall;
use super::session::ProxySessionTracker;
use crate::AppState;

/// Shared state for the proxy, held alongside AppState.
pub struct ProxyState {
    pub tracker: Arc<ProxySessionTracker>,
}

impl ProxyState {
    pub fn new() -> Self {
        ProxyState {
            tracker: Arc::new(ProxySessionTracker::new()),
        }
    }
}

/// Main proxy handler for POST /v1/messages
pub async fn proxy_messages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let proxy_tracker = match &state.proxy_state {
        Some(p) => Arc::clone(&p.tracker),
        None => {
            return (StatusCode::SERVICE_UNAVAILABLE, "proxy not enabled").into_response();
        }
    };

    // Parse the request body
    let mut body_json: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("proxy: invalid JSON body: {}", e);
            return (
                StatusCode::BAD_REQUEST,
                format!("invalid JSON: {}", e),
            )
                .into_response();
        }
    };

    // Check if this is a valid messages request
    let has_messages = body_json
        .get("messages")
        .and_then(|m| m.as_array())
        .is_some();
    let is_streaming = body_json
        .get("stream")
        .and_then(|s| s.as_bool())
        .unwrap_or(false);

    // Generate a stable session ID from the conversation
    let session_id = derive_session_id(&body_json);

    // Run recall pipeline (injects context into system prompt)
    if has_messages {
        let recall_result = run_recall(
            Arc::clone(&state),
            Arc::clone(&proxy_tracker),
            &mut body_json,
            &session_id,
        )
        .await;

        if let Some(ref injected) = recall_result {
            tracing::debug!("proxy: injected {} chars of context", injected.len());
        }
    }

    // Save messages for capture pipeline (before we send the modified body)
    let messages_for_capture = if has_messages && state.config.proxy.capture.enabled {
        body_json
            .get("messages")
            .and_then(|m| m.as_array())
            .cloned()
    } else {
        None
    };

    // Build the upstream request to Anthropic
    let anthropic_url = format!("{}/v1/messages", state.config.proxy.anthropic_url);

    let mut upstream_req = state
        .http_client
        .post(&anthropic_url)
        .body(serde_json::to_vec(&body_json).unwrap_or_default());

    // Forward relevant headers (API key, version, content type)
    for (name, value) in headers.iter() {
        let name_str = name.as_str().to_lowercase();
        match name_str.as_str() {
            "x-api-key" | "anthropic-version" | "anthropic-beta" | "content-type" => {
                upstream_req = upstream_req.header(name.clone(), value.clone());
            }
            "authorization" => {
                // Forward Authorization header too (some clients use Bearer instead of x-api-key)
                upstream_req = upstream_req.header(name.clone(), value.clone());
            }
            _ => {} // Skip other headers (host, connection, etc.)
        }
    }

    // Send the request to Anthropic
    let upstream_resp = match upstream_req.send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("proxy: upstream request failed: {}", e);
            return (
                StatusCode::BAD_GATEWAY,
                format!("upstream error: {}", e),
            )
                .into_response();
        }
    };

    let resp_status = upstream_resp.status();
    let resp_headers = upstream_resp.headers().clone();

    // Only run capture if request succeeded
    let should_capture = resp_status.is_success() && messages_for_capture.is_some();

    if is_streaming {
        stream_response(
            Arc::clone(&state),
            Arc::clone(&proxy_tracker),
            upstream_resp,
            resp_status,
            resp_headers,
            session_id,
            messages_for_capture,
            should_capture,
        )
        .await
    } else {
        non_stream_response(
            Arc::clone(&state),
            Arc::clone(&proxy_tracker),
            upstream_resp,
            resp_status,
            resp_headers,
            session_id,
            messages_for_capture,
            should_capture,
        )
        .await
    }
}

/// Handle streaming SSE response from Anthropic
async fn stream_response(
    state: Arc<AppState>,
    proxy_tracker: Arc<ProxySessionTracker>,
    upstream_resp: reqwest::Response,
    status: reqwest::StatusCode,
    resp_headers: HeaderMap,
    session_id: String,
    messages_for_capture: Option<Vec<serde_json::Value>>,
    should_capture: bool,
) -> Response {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(64);

    let capture_state = Arc::clone(&state);
    let capture_tracker = Arc::clone(&proxy_tracker);
    let capture_session = session_id.clone();

    // Spawn a task to read upstream and forward to the channel
    tokio::spawn(async move {
        let mut stream = upstream_resp.bytes_stream();

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    if tx.send(Ok(bytes)).await.is_err() {
                        break; // Client disconnected
                    }
                }
                Err(e) => {
                    let io_err: std::io::Error = std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    );
                    let _ = tx.send(Err(io_err)).await;
                    break;
                }
            }
        }

        // After stream completes, run capture pipeline
        if should_capture {
            if let Some(messages) = messages_for_capture {
                tokio::spawn(async move {
                    run_capture(
                        capture_state,
                        capture_tracker,
                        capture_session,
                        messages,
                    )
                    .await;
                });
            }
        }
    });

    // Build the response
    let body = Body::from_stream(ReceiverStream::new(rx));

    let mut response = Response::builder()
        .status(StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK));

    // Forward response headers
    for (name, value) in resp_headers.iter() {
        let name_str = name.as_str().to_lowercase();
        match name_str.as_str() {
            "content-type" | "x-request-id" | "request-id" => {
                response = response.header(name.clone(), value.clone());
            }
            _ => {}
        }
    }

    response.body(body).unwrap_or_else(|_| {
        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from("failed to build response"))
            .unwrap()
    })
}

/// Handle non-streaming response from Anthropic
async fn non_stream_response(
    state: Arc<AppState>,
    proxy_tracker: Arc<ProxySessionTracker>,
    upstream_resp: reqwest::Response,
    status: reqwest::StatusCode,
    resp_headers: HeaderMap,
    session_id: String,
    messages_for_capture: Option<Vec<serde_json::Value>>,
    should_capture: bool,
) -> Response {
    // Read full response body
    let body_bytes = match upstream_resp.bytes().await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("failed to read upstream response: {}", e),
            )
                .into_response();
        }
    };

    // Spawn capture pipeline after response
    if should_capture {
        if let Some(messages) = messages_for_capture {
            let capture_state = Arc::clone(&state);
            let capture_tracker = Arc::clone(&proxy_tracker);
            tokio::spawn(async move {
                run_capture(capture_state, capture_tracker, session_id, messages).await;
            });
        }
    }

    // Build response
    let mut response = Response::builder()
        .status(StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK));

    for (name, value) in resp_headers.iter() {
        let name_str = name.as_str().to_lowercase();
        match name_str.as_str() {
            "content-type" | "x-request-id" | "request-id" => {
                response = response.header(name.clone(), value.clone());
            }
            _ => {}
        }
    }

    response
        .body(Body::from(body_bytes))
        .unwrap_or_else(|_| {
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("failed to build response"))
                .unwrap()
        })
}

/// Proxy stats endpoint
pub async fn proxy_stats(
    State(state): State<Arc<AppState>>,
) -> axum::Json<serde_json::Value> {
    let stats = match &state.proxy_state {
        Some(p) => p.tracker.stats().await,
        None => serde_json::json!({"error": "proxy not enabled"}),
    };
    axum::Json(serde_json::json!({
        "ok": true,
        "proxy": stats,
    }))
}

/// Derive a stable session ID from the conversation.
/// Uses a hash of the first message to identify the session.
/// This is a heuristic -- Claude Code may not send a session ID.
fn derive_session_id(body: &serde_json::Value) -> String {
    let messages = match body.get("messages").and_then(|m| m.as_array()) {
        Some(msgs) if !msgs.is_empty() => msgs,
        _ => return format!("unknown-{}", uuid::Uuid::new_v4()),
    };

    // Hash the first user message + model to get a stable session key
    let mut hasher = Sha256::new();

    if let Some(first) = messages.first() {
        if let Some(content) = first.get("content") {
            hasher.update(content.to_string().as_bytes());
        }
    }

    if let Some(model) = body.get("model").and_then(|m| m.as_str()) {
        hasher.update(model.as_bytes());
    }

    let hash = hasher.finalize();
    let hex_str: String = hash[..8].iter().map(|b| format!("{:02x}", b)).collect();
    format!("proxy-{}", hex_str)
}
