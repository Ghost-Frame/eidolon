use std::sync::Arc;
use serde_json::json;

use crate::AppState;
use crate::session::SessionStatus;

fn store_request(state: &Arc<AppState>, url: &str, body: serde_json::Value) -> reqwest::RequestBuilder {
    let req = state.http_client.post(url).json(&body);
    if let Some(ref key) = state.config.engram.api_key {
        req.header("Authorization", format!("Bearer {}", key))
    } else {
        req
    }
}

pub async fn absorb_session(state: Arc<AppState>, session_id: String) {
    let (task, output_buffer, status, corrections, agent, short_id) = {
        let sessions = state.sessions.lock().await;
        match sessions.get_session(&session_id) {
            Some(s) => (
                s.task.clone(),
                s.output_buffer.clone(),
                s.status.clone(),
                s.corrections,
                s.agent.clone(),
                s.short_id().to_string(),
            ),
            None => {
                tracing::warn!("absorber: session {} not found", session_id);
                return;
            }
        }
    };

    let outcome = match status {
        SessionStatus::Completed => "succeeded",
        SessionStatus::Failed => "failed",
        SessionStatus::Killed => "killed",
        _ => "unknown",
    };

    let importance = match status {
        SessionStatus::Completed => 6,
        _ => 7,
    };

    // Build summary
    let summary = format!(
        "Eidolon session ({}) for task \"{}\": {}. Agent: {}. Corrections: {}.",
        short_id,
        task.chars().take(100).collect::<String>(),
        outcome,
        agent,
        corrections,
    );

    // Store session summary to Engram
    let store_url = format!("{}/store", state.config.engram.url);
    let store_result = store_request(&state, &store_url, json!({
            "content": summary,
            "category": "task",
            "source": "eidolon-daemon",
            "importance": importance,
        }))
        .send()
        .await;

    match store_result {
        Ok(resp) if resp.status().is_success() => {
            tracing::info!("absorber: stored session summary for {}", short_id);
        }
        Ok(resp) => {
            tracing::warn!("absorber: Engram store returned {}", resp.status());
        }
        Err(e) => {
            tracing::warn!("absorber: Engram store failed: {}", e);
        }
    }

    // Scan for gate blocks in output
    let blocked_lines: Vec<&str> = output_buffer.iter()
        .filter(|line| {
            let lower = line.to_lowercase();
            lower.contains("blocked") || lower.contains("gate: block")
        })
        .map(|s| s.as_str())
        .collect();

    for line in blocked_lines.iter().take(5) {
        let block_content = format!("Gate blocked action in session {}: {}", short_id, line);
        let _ = store_request(&state, &store_url, json!({
                "content": block_content,
                "category": "issue",
                "source": "eidolon-daemon",
                "importance": 8,
            }))
            .send()
            .await;
    }

    // Scan output for key discoveries
    let discovery_keywords = ["discovered", "fixed", "deployed", "created", "updated", "error", "failed"];
    let discovery_lines: Vec<String> = output_buffer.iter()
        .filter(|line| {
            let lower = line.to_lowercase();
            discovery_keywords.iter().any(|kw| lower.contains(kw))
        })
        .take(10)
        .cloned()
        .collect();

    if !discovery_lines.is_empty() {
        let discoveries = discovery_lines.join("\n");
        let discovery_content = format!(
            "Session {} discoveries for task \"{}\": {}",
            short_id,
            task.chars().take(80).collect::<String>(),
            discoveries,
        );
        let _ = store_request(&state, &store_url, json!({
                "content": discovery_content,
                "category": "discovery",
                "source": "eidolon-daemon",
                "importance": 5,
            }))
            .send()
            .await;
        tracing::info!("absorber: stored {} discovery lines for session {}", discovery_lines.len(), short_id);
    }
}
