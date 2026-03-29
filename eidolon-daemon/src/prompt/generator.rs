use std::sync::Arc;
use serde_json::json;

use crate::AppState;
use super::templates;

// Credential scrubbing patterns -- never leak secrets into prompts
const SCRUB_PATTERNS: &[&str] = &[
    "password", "passwd", "secret", "token", "api_key", "apikey",
    "private_key", "bearer", "authorization", "credential",
];

#[allow(dead_code)]
pub struct MemorySummary {
    pub id: i64,
    pub content: String,
    pub category: String,
    pub activation: f32,
}

fn scrub_credentials(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for line in text.lines() {
        let line_lower = line.to_lowercase();
        let is_credential_line = SCRUB_PATTERNS.iter().any(|pat| line_lower.contains(pat));

        if is_credential_line {
            if line.contains('=') || (line.contains(':') && !line.contains("://") && !line.contains("path")) {
                result.push_str("[CREDENTIAL REDACTED -- use credential manager]\n");
                continue;
            }
        }
        result.push_str(line);
        result.push('\n');
    }
    result
}

async fn search_engram(
    state: &Arc<AppState>,
    query: &str,
    limit: u32,
) -> Vec<MemorySummary> {
    let engram_url = &state.config.engram.url;
    let search_url = format!("{}/search", engram_url);
    let mut req = state.http_client
        .post(&search_url)
        .json(&json!({"query": query, "limit": limit}));
    if let Some(ref key) = state.config.engram.api_key {
        req = req.header("Authorization", format!("Bearer {}", key));
    }
    match req.send().await {
        Ok(resp) if resp.status().is_success() => {
            resp.json::<serde_json::Value>().await
                .ok()
                .and_then(|v| v["results"].as_array().map(|arr| {
                    arr.iter().filter_map(|m| {
                        Some(MemorySummary {
                            id: m["id"].as_i64().unwrap_or(0),
                            content: scrub_credentials(m["content"].as_str()?),
                            category: m["category"].as_str()?.to_string(),
                            activation: m["score"].as_f64().unwrap_or(0.5) as f32,
                        })
                    }).collect()
                }))
                .unwrap_or_default()
        }
        _ => {
            tracing::warn!("Engram /search unavailable for query: {}", query);
            vec![]
        }
    }
}

pub async fn generate_prompt(
    state: &Arc<AppState>,
    task: &str,
    _agent_type: &str,
) -> String {
    // Search 1: Task-specific memories
    let task_memories = search_engram(state, task, 12).await;

    // Search 2: Infrastructure context (always include)
    let infra_memories = search_engram(state, "server infrastructure deployment SSH", 8).await;

    // Search 3: Safety constraints and rules (always include)
    let safety_memories = search_engram(state, "constraints rules safety blocked", 6).await;

    // Search 4: Recent issues/failures related to task
    let failure_query = format!("failure problem error issue {}", task);
    let failure_memories = search_engram(state, &failure_query, 5).await;

    let engram_url = &state.config.engram.url;

    templates::build_living_prompt(
        task,
        &task_memories,
        &infra_memories,
        &safety_memories,
        &failure_memories,
        engram_url,
    )
}
