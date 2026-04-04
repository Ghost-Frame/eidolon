use std::sync::Arc;
use ndarray::Array1;
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

/// Absorb a memory directly into the in-process brain.
/// Gets embedding from Engram /embed, constructs BrainMemory, calls Brain::absorb_new().
pub async fn absorb_to_brain(
    state: &Arc<AppState>,
    content: &str,
    category: &str,
    importance: i32,
) {
    let embedding = match crate::embed_text(
        &state.http_client,
        &state.config.engram.url,
        state.config.engram.api_key.as_deref(),
        content,
    ).await {
        Some(e) if !e.is_empty() => e,
        _ => {
            tracing::warn!("absorber: embed failed, skipping brain absorption");
            return;
        }
    };

    // Generate a unique ID using UUID v4
    let id = (uuid::Uuid::new_v4().as_u128() as i64).abs();

    let memory = eidolon_lib::types::BrainMemory {
        id,
        content: content.to_string(),
        category: category.to_string(),
        source: "eidolon-daemon".to_string(),
        importance,
        created_at: chrono::Utc::now().to_rfc3339(),
        embedding,
        pattern: Array1::zeros(0), // absorb_new() will PCA-project from embedding
        activation: 0.0,
        last_activated: chrono::Utc::now().timestamp() as f64,
        access_count: 0,
        decay_factor: 1.0,
        tags: vec![],
    };

    let mut brain = state.brain.lock().await;
    brain.absorb_new(memory);
    tracing::info!("absorber: absorbed memory id={} into brain ({})", id, category);
}

pub async fn absorb_session(state: Arc<AppState>, session_id: String) {
    let (task, output_buffer, status, corrections, agent, short_id) = {
        let sessions = state.sessions.lock().await;
        match sessions.get_session(&session_id, None) {
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
        SessionStatus::TimedOut => "timed_out",
        _ => "unknown",
    };

    let importance = match status {
        SessionStatus::Completed => 6,
        _ => 7,
    };

    // Build summary (scrub tier-3 secrets before storing)
    let summary = {
        let raw = format!(
            "Eidolon session ({}) for task \"{}\": {}. Agent: {}. Corrections: {}.",
            short_id,
            task.chars().take(100).collect::<String>(),
            outcome,
            agent,
            corrections,
        );
        let scrub = state.scrub_registry.lock().await;
        scrub.scrub(&session_id, &raw)
    };

    // 1. Absorb session summary into brain
    absorb_to_brain(&state, &summary, "task", importance).await;

    // 2. Also store to Engram for cross-agent visibility
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

    // 3. Absorb gate blocks as strong correction signals into brain
    let blocked_lines: Vec<String> = output_buffer.iter()
        .filter(|line| {
            let lower = line.to_lowercase();
            lower.contains("blocked") || lower.contains("gate: block")
        })
        .take(5)
        .cloned()
        .collect();

    for line in &blocked_lines {
        let block_content = {
            let raw = format!("Gate blocked action in session {}: {}", short_id, line);
            let scrub = state.scrub_registry.lock().await;
            scrub.scrub(&session_id, &raw)
        };
        // Absorb blocks with high importance -- these are correction signals
        absorb_to_brain(&state, &block_content, "issue", 8).await;

        // Also to Engram
        let _ = store_request(&state, &store_url, json!({
                "content": block_content,
                "category": "issue",
                "source": "eidolon-daemon",
                "importance": 8,
            }))
            .send()
            .await;
    }

    // 4. Extract and absorb key discoveries
    let discovery_keywords = ["discovered", "fixed", "deployed", "created", "updated"];
    let discovery_lines: Vec<String> = output_buffer.iter()
        .filter(|line| {
            let lower = line.to_lowercase();
            discovery_keywords.iter().any(|kw| lower.contains(kw))
                && !lower.contains("blocked")
                && !lower.contains("[stderr]")
        })
        .take(10)
        .cloned()
        .collect();

    if !discovery_lines.is_empty() {
        let discoveries = discovery_lines.join("\n");
        let discovery_content = {
            let raw = format!(
                "Session {} discoveries for task \"{}\": {}",
                short_id,
                task.chars().take(80).collect::<String>(),
                discoveries,
            );
            let scrub = state.scrub_registry.lock().await;
            scrub.scrub(&session_id, &raw)
        };
        absorb_to_brain(&state, &discovery_content, "discovery", 5).await;

        let _ = store_request(&state, &store_url, json!({
                "content": discovery_content,
                "category": "discovery",
                "source": "eidolon-daemon",
                "importance": 5,
            }))
            .send()
            .await;
        tracing::info!("absorber: absorbed {} discovery lines for session {}", discovery_lines.len(), short_id);
    }

    // Clean up scrub tracking for this session
    {
        let mut scrub = state.scrub_registry.lock().await;
        scrub.remove_session(&session_id);
    }
}
