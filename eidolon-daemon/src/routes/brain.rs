use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::AppState;

pub async fn brain_stats(
    State(state): State<Arc<AppState>>,
) -> Json<Value> {
    let brain = state.brain.lock().await;
    let stats = brain.get_stats();
    Json(json!({
        "ok": true,
        "stats": stats,
    }))
}

#[derive(Debug, Deserialize)]
pub struct BrainQueryRequest {
    pub query: String,
    pub top_k: Option<usize>,
    pub beta: Option<f32>,
    pub spread_hops: Option<usize>,
    #[cfg(feature = "reasoning")]
    pub reasoning: Option<bool>,
}

pub async fn brain_query(
    State(state): State<Arc<AppState>>,
    axum::Extension(user): axum::Extension<crate::UserIdentity>,
    Json(req): Json<BrainQueryRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if req.query.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "query must not be empty"})),
        ));
    }

    if req.query.len() > 4096 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": "query too long (max 4096 chars)"})),
        ));
    }

    tracing::info!("brain_query: user={} query_len={}", user.0, req.query.len());

    // Call Engram /embed to get embedding vector
    let embedding: Vec<f32> = match crate::embed_text(
        &state.http_client,
        &state.config.engram.url,
        state.config.engram.api_key.as_deref(),
        &req.query,
    ).await {
        Some(v) if !v.is_empty() => v,
        _ => {
            tracing::warn!("Engram /embed unavailable for brain_query, returning empty result");
            return Ok(Json(json!({
                "ok": true,
                "note": "Engram embed unavailable",
                "result": null,
            })));
        }
    };

    if embedding.is_empty() {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Engram returned empty embedding"})),
        ));
    }

    let top_k = req.top_k.unwrap_or(10);
    let beta = req.beta.unwrap_or(8.0);
    let spread_hops = req.spread_hops.unwrap_or(2);

    let mut brain = state.brain.lock().await;
    let result = brain.query(&embedding, top_k, beta, spread_hops);

    Ok(Json(json!({
        "ok": true,
        "result": result,
        "user": user.0,
    })))
}

pub async fn brain_dream(
    State(state): State<Arc<AppState>>,
) -> Json<Value> {
    let mut brain = state.brain.lock().await;
    let result = brain.run_dream_cycle();
    tracing::info!(
        "manual dream cycle: replayed={} merged={} pruned={} discovered={} resolved={} ({}ms)",
        result.replayed, result.merged, result.pruned_patterns,
        result.discovered, result.resolved, result.cycle_time_ms
    );
    Json(json!({
        "ok": true,
        "result": result,
    }))
}
