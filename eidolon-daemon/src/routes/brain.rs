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
}

pub async fn brain_query(
    State(state): State<Arc<AppState>>,
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

    // Call Engram /embed to get embedding vector
    let embed_url = format!("{}/embed", state.config.engram.url);
    let embed_resp = state.http_client
        .post(&embed_url)
        .json(&json!({"text": req.query}))
        .send()
        .await;

    let embedding: Vec<f32> = match embed_resp {
        Ok(resp) if resp.status().is_success() => {
            resp.json::<serde_json::Value>().await
                .ok()
                .and_then(|v| serde_json::from_value(v["embedding"].clone()).ok())
                .unwrap_or_default()
        }
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
