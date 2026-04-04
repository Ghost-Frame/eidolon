use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct ActivityRequest {
    pub agent: String,
    pub action: String,
    pub summary: String,
    pub project: Option<String>,
    pub details: Option<Value>,
}

/// POST /activity - Unified Syntheos fan-out gateway.
///
/// Agents POST a single activity report. Eidolon fans out to:
/// - Chiasm (task tracking)
/// - Axon (event bus)
/// - Broca (action log)
/// - Engram (memory storage, on completions)
/// - Brain (local absorption, always)
/// - Soma (agent registry + heartbeat)
/// - Thymus (drift events + session quality)
///
/// All fan-out is best-effort. Individual failures are logged but
/// do not fail the request.
pub async fn post_activity(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ActivityRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // - Validate --
    if req.agent.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "agent is required"}))));
    }
    if req.agent.len() > 100 {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "agent exceeds 100 characters"}))));
    }
    if req.action.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "action is required"}))));
    }
    if req.action.len() > 100 {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "action exceeds 100 characters"}))));
    }
    if req.summary.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "summary is required"}))));
    }
    if req.summary.len() > 10_000 {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "summary exceeds 10000 characters"}))));
    }
    if let Some(ref details) = req.details {
        let details_size = serde_json::to_string(details).map(|s| s.len()).unwrap_or(0);
        if details_size > 50_000 {
            return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "details exceeds 50KB"}))));
        }
    }

    let engram_url = &state.config.engram.url;
    let axon_base = state.config.engram.axon_url.as_deref()
        .map(|u| u.trim_end_matches("/publish").to_string())
        .unwrap_or_else(|| format!("{}/axon", engram_url));
    let engram_key = state.config.engram.api_key.clone().unwrap_or_default();
    let http = &state.http_client;
    let project = req.project.as_deref().unwrap_or("unknown");
    let summary_short: String = req.summary.chars().take(500).collect();

    // - Chiasm: task tracking (needs sequential query-then-act) --
    let chiasm_result = fanout_chiasm(
        http, engram_url, &engram_key,
        &req.agent, &req.action, project, &summary_short,
    ).await;

    // - Parallel fan-out: Axon + Broca + Brain + Engram --
    let axon_channel = match req.action.as_str() {
        "task.blocked" | "error.raised" => "alerts",
        a if a.starts_with("task.") => "tasks",
        a if a.starts_with("drift.") || a.starts_with("session.") => "quality",
        _ => "system",
    };
    let axon_type = req.action.clone();

    let axon_fut = fanout_axon(
        http, &axon_base, &engram_key,
        &req.agent, axon_channel, &axon_type, &summary_short, &req.details,
    );

    let broca_fut = fanout_broca(
        http, engram_url, &engram_key,
        &req.agent, &req.action, &summary_short,
    );

    let brain_content = format!(
        "Agent {} [{}] (project: {}): {}",
        req.agent, req.action, project, req.summary
    );
    let brain_category = if req.action.starts_with("task.") { "task" } else { "activity" };
    let brain_importance = match req.action.as_str() {
        "task.completed" => 6,
        "task.blocked" | "error.raised" => 7,
        _ => 4,
    };
    let brain_fut = crate::absorber::absorb_to_brain(
        &state, &brain_content, brain_category, brain_importance,
    );

    // Only store to Engram on completions and errors (not every progress tick)
    let store_to_engram = matches!(
        req.action.as_str(),
        "task.completed" | "task.blocked" | "error.raised"
    );
    let engram_fut = async {
        if store_to_engram {
            fanout_engram(
                http, engram_url, &engram_key,
                &req.agent, &summary_short, brain_category,
            ).await
        } else {
            "skipped".to_string()
        }
    };

    let thymus_fut = async {
        if req.action.starts_with("drift.") || req.action.starts_with("session.") {
            fanout_thymus(
                http, engram_url, &engram_key,
                &req.agent, &req.action, &summary_short, &req.details,
            ).await
        } else {
            "skipped".to_string()
        }
    };

    let soma_fut = fanout_soma(
        http, engram_url, &engram_key,
        &req.agent, &req.action,
    );

    let (axon_result, broca_result, _, engram_result, thymus_result, soma_result) =
        tokio::join!(axon_fut, broca_fut, brain_fut, engram_fut, thymus_fut, soma_fut);

    Ok(Json(json!({
        "ok": true,
        "fanout": {
            "chiasm": chiasm_result,
            "axon": axon_result,
            "broca": broca_result,
            "brain": "absorbed",
            "engram": engram_result,
            "thymus": thymus_result,
            "soma": soma_result,
        }
    })))
}

// - Chiasm fan-out: find-or-create task, then update --

async fn fanout_chiasm(
    http: &reqwest::Client,
    engram_url: &str,
    engram_key: &str,
    agent: &str,
    action: &str,
    project: &str,
    summary: &str,
) -> Value {
    let auth = format!("Bearer {}", engram_key);

    // For non-task actions, skip Chiasm entirely
    if !action.starts_with("task.") {
        return json!("skipped");
    }

    let chiasm_status = match action {
        "task.started" => "active",
        "task.progress" => "active",
        "task.completed" => "completed",
        "task.blocked" => "blocked",
        _ => "active",
    };

    // Try to find existing active task for this agent+project
    let find_url = reqwest::Url::parse_with_params(
        &format!("{}/tasks", engram_url),
        &[("agent", agent), ("project", project), ("status", "active"), ("limit", "1")],
    ).map(|u| u.to_string())
    .unwrap_or_else(|_| format!("{}/tasks?agent={}&project={}&status=active&limit=1", engram_url, agent, project));

    let existing_task_id = match http.get(&find_url)
        .header("Authorization", &auth)
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            resp.json::<Value>().await.ok().and_then(|v| {
                v.get("tasks")
                    .or(v.as_array().map(|_| &v))
                    .and_then(|tasks| {
                        let arr = tasks.as_array()?;
                        arr.first()?.get("id")?.as_i64()
                    })
            })
        }
        _ => None,
    };

    match (action, existing_task_id) {
        // task.started always creates new
        ("task.started", _) => {
            match create_chiasm_task(http, engram_url, &auth, agent, project, summary).await {
                Some(id) => json!({"created": id}),
                None => json!("create_failed"),
            }
        }
        // Has existing task - update it
        (_, Some(task_id)) => {
            match update_chiasm_task(http, engram_url, &auth, task_id, chiasm_status, summary).await {
                true => json!({"updated": task_id}),
                false => json!("update_failed"),
            }
        }
        // No existing task - auto-create then we're done
        (_, None) => {
            match create_chiasm_task(http, engram_url, &auth, agent, project, summary).await {
                Some(id) => {
                    // If not "started", update status after creation
                    if chiasm_status != "active" {
                        update_chiasm_task(http, engram_url, &auth, id, chiasm_status, summary).await;
                    }
                    json!({"auto_created": id})
                }
                None => json!("create_failed"),
            }
        }
    }
}

async fn create_chiasm_task(
    http: &reqwest::Client,
    engram_url: &str,
    auth: &str,
    agent: &str,
    project: &str,
    title: &str,
) -> Option<i64> {
    let url = format!("{}/tasks", engram_url);
    let resp = http.post(&url)
        .header("Authorization", auth)
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(3))
        .json(&json!({
            "agent": agent,
            "project": project,
            "title": title,
        }))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        tracing::warn!("activity: chiasm create failed: {}", resp.status());
        return None;
    }
    let body: Value = resp.json().await.ok()?;
    body.get("id").and_then(|v| v.as_i64())
}

async fn update_chiasm_task(
    http: &reqwest::Client,
    engram_url: &str,
    auth: &str,
    task_id: i64,
    status: &str,
    summary: &str,
) -> bool {
    let url = format!("{}/tasks/{}", engram_url, task_id);
    match http.patch(&url)
        .header("Authorization", auth)
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(3))
        .json(&json!({
            "status": status,
            "summary": summary,
        }))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => true,
        Ok(resp) => {
            tracing::warn!("activity: chiasm update task {} failed: {}", task_id, resp.status());
            false
        }
        Err(e) => {
            tracing::warn!("activity: chiasm update task {} error: {}", task_id, e);
            false
        }
    }
}

// - Axon fan-out --

async fn fanout_axon(
    http: &reqwest::Client,
    axon_base: &str,
    engram_key: &str,
    agent: &str,
    channel: &str,
    event_type: &str,
    summary: &str,
    details: &Option<Value>,
) -> String {
    let url = format!("{}/publish", axon_base);
    let mut payload = json!({
        "agent": agent,
        "summary": summary,
    });
    if let Some(d) = details {
        payload.as_object_mut().unwrap().insert("details".to_string(), d.clone());
    }

    match http.post(&url)
        .header("Authorization", format!("Bearer {}", engram_key))
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(3))
        .json(&json!({
            "channel": channel,
            "source": agent,
            "type": event_type,
            "payload": payload,
        }))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => "published".to_string(),
        Ok(resp) => {
            tracing::warn!("activity: axon publish failed: {}", resp.status());
            "failed".to_string()
        }
        Err(e) => {
            tracing::warn!("activity: axon publish error: {}", e);
            "failed".to_string()
        }
    }
}

// - Broca fan-out --

async fn fanout_broca(
    http: &reqwest::Client,
    engram_url: &str,
    engram_key: &str,
    agent: &str,
    action: &str,
    summary: &str,
) -> String {
    let url = format!("{}/broca/actions", engram_url);
    match http.post(&url)
        .header("Authorization", format!("Bearer {}", engram_key))
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(3))
        .json(&json!({
            "agent": agent,
            "service": "eidolon",
            "action": action,
            "payload": { "summary": summary },
        }))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => "logged".to_string(),
        Ok(resp) => {
            tracing::warn!("activity: broca log failed: {}", resp.status());
            "failed".to_string()
        }
        Err(e) => {
            tracing::warn!("activity: broca log error: {}", e);
            "failed".to_string()
        }
    }
}

// - Engram fan-out --

async fn fanout_engram(
    http: &reqwest::Client,
    engram_url: &str,
    engram_key: &str,
    agent: &str,
    summary: &str,
    category: &str,
) -> String {
    let url = format!("{}/store", engram_url);
    let source = format!("{}-via-eidolon", agent);
    match http.post(&url)
        .header("Authorization", format!("Bearer {}", engram_key))
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(3))
        .json(&json!({
            "content": summary,
            "category": category,
            "source": source,
            "importance": 6,
        }))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => "stored".to_string(),
        Ok(resp) => {
            tracing::warn!("activity: engram store failed: {}", resp.status());
            "failed".to_string()
        }
        Err(e) => {
            tracing::warn!("activity: engram store error: {}", e);
            "failed".to_string()
        }
    }
}

// -- Soma fan-out (agent registry + heartbeat) --

async fn fanout_soma(
    http: &reqwest::Client,
    engram_url: &str,
    engram_key: &str,
    agent: &str,
    action: &str,
) -> String {
    let auth = format!("Bearer {}", engram_key);
    let soma_base = format!("{}/soma", engram_url);

    // Determine heartbeat status from action
    let status = match action {
        "task.started" => "online",
        "task.progress" => "online",
        "task.completed" => "online",
        "task.blocked" => "online",
        "error.raised" => "error",
        _ => "online",
    };

    // Try to find existing agent by name
    let list_url = format!("{}/agents", soma_base);
    let existing_id = match http.get(&list_url)
        .header("Authorization", &auth)
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            resp.json::<Value>().await.ok().and_then(|v| {
                let agents = v.as_array()?;
                agents.iter().find(|a| {
                    a.get("name").and_then(|n| n.as_str()) == Some(agent)
                })?.get("id").and_then(|id| id.as_i64().map(|i| i.to_string()).or_else(|| id.as_str().map(String::from)))
            })
        }
        _ => None,
    };

    let agent_id = match existing_id {
        Some(id) => id,
        None => {
            // Register new agent
            match http.post(&format!("{}/agents", soma_base))
                .header("Authorization", &auth)
                .header("Content-Type", "application/json")
                .timeout(std::time::Duration::from_secs(3))
                .json(&json!({
                    "name": agent,
                    "type": "cli",
                }))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<Value>().await.ok().and_then(|v| {
                        v.get("id").and_then(|id| id.as_i64().map(|i| i.to_string()).or_else(|| id.as_str().map(String::from)))
                    }) {
                        Some(id) => id,
                        None => return "register_failed".to_string(),
                    }
                }
                _ => return "register_failed".to_string(),
            }
        }
    };

    // Send heartbeat
    match http.post(&format!("{}/agents/{}/heartbeat", soma_base, agent_id))
        .header("Authorization", &auth)
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(3))
        .json(&json!({ "status": status }))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => "heartbeat".to_string(),
        Ok(resp) => {
            tracing::warn!("activity: soma heartbeat failed: {}", resp.status());
            "failed".to_string()
        }
        Err(e) => {
            tracing::warn!("activity: soma heartbeat error: {}", e);
            "failed".to_string()
        }
    }
}

// -- Thymus fan-out (drift events + session quality) --

async fn fanout_thymus(
    http: &reqwest::Client,
    engram_url: &str,
    engram_key: &str,
    agent: &str,
    action: &str,
    summary: &str,
    details: &Option<Value>,
) -> String {
    let auth = format!("Bearer {}", engram_key);

    match action {
        "drift.detected" => {
            // Extract drift details from the details field or summary
            let (drift_type, severity, signal) = if let Some(d) = details {
                (
                    d.get("drift_type").and_then(|v| v.as_str()).unwrap_or("framework"),
                    d.get("severity").and_then(|v| v.as_str()).unwrap_or("low"),
                    d.get("signal").and_then(|v| v.as_str()).unwrap_or(summary),
                )
            } else {
                ("framework", "low", summary)
            };

            let url = format!("{}/thymus/drift-events", engram_url);
            match http.post(&url)
                .header("Authorization", &auth)
                .header("Content-Type", "application/json")
                .timeout(std::time::Duration::from_secs(3))
                .json(&json!({
                    "agent": agent,
                    "drift_type": drift_type,
                    "severity": severity,
                    "signal": signal,
                }))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => "drift_recorded".to_string(),
                Ok(resp) => {
                    tracing::warn!("activity: thymus drift-event failed: {}", resp.status());
                    "failed".to_string()
                }
                Err(e) => {
                    tracing::warn!("activity: thymus drift-event error: {}", e);
                    "failed".to_string()
                }
            }
        }
        "session.quality" => {
            // Post session quality metrics
            let url = format!("{}/thymus/metrics", engram_url);
            let value = details.as_ref()
                .and_then(|d| d.get("score"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.5);
            let tags = details.as_ref()
                .and_then(|d| d.get("tags"))
                .cloned()
                .unwrap_or(json!({}));

            match http.post(&url)
                .header("Authorization", &auth)
                .header("Content-Type", "application/json")
                .timeout(std::time::Duration::from_secs(3))
                .json(&json!({
                    "agent": agent,
                    "name": "session_compliance",
                    "value": value,
                    "tags": tags,
                }))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => "quality_recorded".to_string(),
                Ok(resp) => {
                    tracing::warn!("activity: thymus session-quality failed: {}", resp.status());
                    "failed".to_string()
                }
                Err(e) => {
                    tracing::warn!("activity: thymus session-quality error: {}", e);
                    "failed".to_string()
                }
            }
        }
        _ => "skipped".to_string(),
    }
}
