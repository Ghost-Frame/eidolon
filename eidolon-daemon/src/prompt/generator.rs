use std::sync::Arc;

use crate::AppState;
use super::templates;

// Credential scrubbing patterns - never leak secrets into prompts
const SCRUB_PATTERNS: &[&str] = &[
    "password", "passwd", "secret", "token", "api_key", "apikey",
    "private_key", "bearer", "authorization", "credential",
];

pub struct MemorySummary {
    pub id: i64,
    pub content: String,
    pub category: String,
    pub activation: f32,
    pub created_at: String,
}

pub struct ContradictionInfo {
    pub winner_content: String,
    pub loser_content: String,
    pub reason: String,
}

fn scrub_credentials(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for line in text.lines() {
        let line_lower = line.to_lowercase();
        let is_credential_line = SCRUB_PATTERNS.iter().any(|pat| line_lower.contains(pat));

        if is_credential_line {
            if line.contains('=') || (line.contains(':') && !line.contains("://") && !line.contains("path")) {
                result.push_str("[CREDENTIAL REDACTED - use credential manager]\n");
                continue;
            }
        }
        result.push_str(line);
        result.push('\n');
    }
    result
}

/// Query the in-process brain for task-relevant context.
/// Embeds the query text via configured provider, then runs Brain::query()
/// for pattern completion with contradiction resolution.
async fn query_brain(
    state: &Arc<AppState>,
    query: &str,
    top_k: usize,
    user: &str,
) -> (Vec<MemorySummary>, Vec<ContradictionInfo>) {
    let embedding = match state.embed_text(query).await {
        Some(e) if !e.is_empty() => e,
        _ => {
            tracing::warn!("brain query: embed failed for prompt query");
            return (vec![], vec![]);
        }
    };

    let mut brain = state.brain.lock().await;
    let result = brain.query(&embedding, top_k, 8.0, 2);

    let user_prefix = format!("user:{}/", user);
    let memories: Vec<MemorySummary> = result.activated.iter()
        .filter(|m| m.category.starts_with(&user_prefix) || m.category.starts_with("system/"))
        .map(|m| {
            MemorySummary {
                id: m.id,
                content: scrub_credentials(&m.content),
                category: m.category.clone(),
                activation: m.activation,
                created_at: m.created_at.clone(),
            }
        }).collect();

    let contradictions: Vec<ContradictionInfo> = result.contradictions.iter().filter_map(|c| {
        let winner = result.activated.iter().find(|m| m.id == c.winner_id)?;
        let loser = result.activated.iter().find(|m| m.id == c.loser_id)?;
        Some(ContradictionInfo {
            winner_content: scrub_credentials(&winner.content),
            loser_content: scrub_credentials(&loser.content),
            reason: c.reason.clone(),
        })
    }).collect();

    (memories, contradictions)
}

pub async fn generate_prompt(
    state: &Arc<AppState>,
    task: &str,
    _agent_type: &str,
    user: &str,
) -> String {
    // Query 1: Task-specific brain recall
    let (task_memories, task_contradictions) = query_brain(state, task, 12, user).await;

    // Query 2: Infrastructure context
    let (infra_memories, _) = query_brain(state, "server infrastructure deployment SSH configuration", 8, user).await;

    // Query 3: Safety and past failures
    let failure_query = format!("failure problem error blocked mistake {}", task);
    let (failure_memories, _) = query_brain(state, &failure_query, 6, user).await;

    let engram_url = &state.config.engram.url;

    templates::build_living_prompt(
        task,
        &task_memories,
        &task_contradictions,
        &infra_memories,
        &failure_memories,
        engram_url,
        &state.config.servers,
        &state.config.safety.rules,
    )
}
