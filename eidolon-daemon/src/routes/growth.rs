//! Growth reflection, observation, and materialization routes

use axum::{
    extract::State,
    http::StatusCode,
    Json,
    response::IntoResponse,
};
use axum::extract::Query;
use serde::Deserialize;
use std::sync::Arc;

use crate::AppState;
use eidolon_lib::growth::{self, ReflectRequest, ReflectResponse};

/// POST /growth/reflect
pub async fn growth_reflect(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ReflectRequest>,
) -> impl IntoResponse {
    let growth_config = &state.config.growth;

    if !growth_config.enabled {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({
            "error": "growth system is disabled"
        }))).into_response();
    }

    if body.service.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
            "error": "service is required"
        }))).into_response();
    }
    if body.context.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
            "error": "context array must not be empty"
        }))).into_response();
    }

    match growth::reflect(
        &state.http_client,
        growth_config,
        &body.service,
        &body.context,
        body.existing_growth.as_deref(),
        body.prompt_override.as_deref(),
    ).await {
        Ok(observation) => {
            // If we got an observation, fan out to /activity as growth.observed
            if let Some(ref obs) = observation {
                let activity_body = serde_json::json!({
                    "agent": body.service,
                    "action": "growth.observed",
                    "summary": obs,
                    "details": {
                        "source": format!("{}-growth", body.service),
                        "category": "growth",
                    }
                });
                // Fire-and-forget self-fanout
                let client = state.http_client.clone();
                let port = state.config.server.port;
                let key = state.config.auth.api_keys.first()
                    .map(|k| k.key.clone())
                    .unwrap_or_default();
                tokio::spawn(async move {
                    let _ = client.post(format!("http://127.0.0.1:{}/activity", port))
                        .bearer_auth(&key)
                        .json(&activity_body)
                        .send()
                        .await;
                });
            }

            Json(ReflectResponse { observation }).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "growth reflection failed");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                "error": e
            }))).into_response()
        }
    }
}

/// Query params for GET /growth/observations
#[derive(Deserialize)]
pub struct ObservationsQuery {
    pub service: Option<String>,
    pub limit: Option<usize>,
    pub since: Option<String>,
}

/// GET /growth/observations
pub async fn growth_observations(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ObservationsQuery>,
) -> impl IntoResponse {
    let limit = query.limit.unwrap_or(20).min(100);
    let source_filter = query.service.as_deref()
        .map(|s| format!("{}-growth", s));

    // Query Engram for growth memories
    let mut url = format!("{}/memories?category=growth&limit={}",
        state.config.engram.url, limit);
    if let Some(ref source) = source_filter {
        url.push_str(&format!("&source={}", source));
    }
    if let Some(ref since) = query.since {
        url.push_str(&format!("&since={}", since));
    }

    let engram_key = state.config.engram.api_key.clone().unwrap_or_default();

    match state.http_client.get(&url)
        .header("Authorization", format!("Bearer {}", engram_key))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            match resp.json::<serde_json::Value>().await {
                Ok(data) => Json(serde_json::json!({
                    "observations": data.get("memories").unwrap_or(&serde_json::json!([]))
                })).into_response(),
                Err(_) => Json(serde_json::json!({ "observations": [] })).into_response(),
            }
        }
        Ok(resp) => {
            let status = resp.status();
            (StatusCode::BAD_GATEWAY, Json(serde_json::json!({
                "error": format!("Engram returned {}", status)
            }))).into_response()
        }
        Err(e) => {
            (StatusCode::BAD_GATEWAY, Json(serde_json::json!({
                "error": format!("Failed to reach Engram: {}", e)
            }))).into_response()
        }
    }
}

/// Query params for GET /growth/materialize
#[derive(Deserialize)]
pub struct MaterializeQuery {
    pub service: Option<String>,
    pub limit: Option<usize>,
    pub max_bytes: Option<usize>,
}

/// GET /growth/materialize
pub async fn growth_materialize(
    State(state): State<Arc<AppState>>,
    Query(query): Query<MaterializeQuery>,
) -> impl IntoResponse {
    let limit = query.limit.unwrap_or(30).min(100);
    let max_bytes = query.max_bytes.unwrap_or(16_000);

    // Query Engram for growth memories, optionally filtered by service
    let source_filter = query.service.as_deref()
        .map(|s| format!("{}-growth", s));

    let mut url = format!("{}/memories?category=growth&limit={}",
        state.config.engram.url, limit);
    if let Some(ref source) = source_filter {
        url.push_str(&format!("&source={}", source));
    }

    let engram_key = state.config.engram.api_key.clone().unwrap_or_default();

    let observations: Vec<(String, String)> = match state.http_client.get(&url)
        .header("Authorization", format!("Bearer {}", engram_key))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            match resp.json::<serde_json::Value>().await {
                Ok(data) => {
                    data.get("memories")
                        .and_then(|m| m.as_array())
                        .map(|arr| arr.iter().filter_map(|m| {
                            let content = m.get("content")?.as_str()?.to_string();
                            let created = m.get("created_at")?.as_str()?
                                .split('T').next()?.to_string();
                            Some((created, content))
                        }).collect())
                        .unwrap_or_default()
                }
                Err(_) => vec![],
            }
        }
        _ => vec![],
    };

    if observations.is_empty() {
        return (StatusCode::OK, "# Growth Log\n\nNo observations yet.\n").into_response();
    }

    let mut output = String::from("# Growth Log\n\nPersonality evolution and learnings accumulated over time.\n\n");
    let mut current_size = output.len();

    for (date, content) in &observations {
        let entry = format!("- [{}] {}\n", date, content);
        if current_size + entry.len() > max_bytes {
            break;
        }
        output.push_str(&entry);
        current_size += entry.len();
    }

    (StatusCode::OK, output).into_response()
}
