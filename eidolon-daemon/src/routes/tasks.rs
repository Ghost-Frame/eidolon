use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::AppState;
use crate::UserIdentity;
use crate::agents::registry::run_agent;

#[derive(Debug, Deserialize)]
pub struct SubmitTaskRequest {
    pub task: String,
    pub agent: Option<String>,
    pub model: Option<String>,
}

pub async fn submit_task(
    State(state): State<Arc<AppState>>,
    axum::Extension(user): axum::Extension<UserIdentity>,
    Json(req): Json<SubmitTaskRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if req.task.len() > 10_000 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "task description exceeds 10000 characters"})),
        ));
    }
    if req.task.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "task must not be empty"})),
        ));
    }

    let agent_name = req.agent
        .clone()
        .unwrap_or_else(|| "claude-code".to_string());

    if !state.config.agents.contains_key(&agent_name) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("unknown agent: {}", agent_name)})),
        ));
    }

    let model = req.model.clone().unwrap_or_else(|| {
        state.config.agents.get(&agent_name)
            .map(|a| a.default_model.clone())
            .unwrap_or_else(|| "sonnet".to_string())
    });

    let session_id = {
        let mut sessions = state.sessions.lock().await;
        sessions.create_session(req.task.clone(), agent_name.clone(), model.clone(), user.0.clone())
    };

    // Auto-register agent with Axon on spawn
    {
        let axon_url = state.config.engram.axon_url.clone()
            .unwrap_or_else(|| format!("{}/axon/publish", state.config.engram.url));
        let axon_key = state.config.engram.api_key.clone().unwrap_or_default();
        let agent_name_axon = agent_name.clone();
        let sid_axon = session_id.clone();
        let http = state.http_client.clone();
        tokio::spawn(async move {
            let _ = http
                .post(&axon_url)
                .header("Authorization", format!("Bearer {}", axon_key))
                .header("Content-Type", "application/json")
                .json(&serde_json::json!({
                    "type": "agent.online",
                    "channel": "system",
                    "source": agent_name_axon,
                    "payload": { "session_id": sid_axon }
                }))
                .send()
                .await;
        });
    }

    let state_clone = Arc::clone(&state);
    let sid = session_id.clone();
    tokio::spawn(async move {
        run_agent(state_clone, sid, agent_name, req.task, model).await;
    });

    Ok(Json(json!({
        "ok": true,
        "session_id": session_id,
        "status": "pending",
    })))
}

pub async fn task_status(
    State(state): State<Arc<AppState>>,
    axum::Extension(user): axum::Extension<UserIdentity>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let sessions = state.sessions.lock().await;
    match sessions.get_session(&id, Some(&user.0)) {
        Some(s) => Ok(Json(s.to_json())),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("session {} not found", id)})),
        )),
    }
}

pub async fn kill_task(
    State(state): State<Arc<AppState>>,
    axum::Extension(user): axum::Extension<UserIdentity>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let mut sessions = state.sessions.lock().await;
    match sessions.kill_session(&id, Some(&user.0)) {
        Ok(()) => Ok(Json(json!({"ok": true, "session_id": id, "status": "killed"}))),
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": e})),
        )),
    }
}
