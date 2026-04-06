use axum::{
    extract::State,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::AppState;
use crate::prompt::generator;

#[derive(Debug, Deserialize)]
pub struct GeneratePromptRequest {
    pub task: String,
    pub agent: Option<String>,
}

pub async fn generate_prompt(
    State(state): State<Arc<AppState>>,
    axum::Extension(user): axum::Extension<crate::UserIdentity>,
    Json(req): Json<GeneratePromptRequest>,
) -> Json<Value> {
    let agent = req.agent.as_deref().unwrap_or("claude-code");
    tracing::info!("prompt/generate: user={} task_len={} agent={}", user.0, req.task.len(), agent);
    let prompt = generator::generate_prompt(&state, &req.task, agent, &user.0).await;
    Json(json!({
        "ok": true,
        "prompt": prompt,
        "user": user.0,
    }))
}
