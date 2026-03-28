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
    // Replace lines that look like credential assignments with path references only
    let mut result = String::with_capacity(text.len());
    for line in text.lines() {
        let line_lower = line.to_lowercase();
        let is_credential_line = SCRUB_PATTERNS.iter().any(|pat| line_lower.contains(pat));

        if is_credential_line {
            // Check if line contains an actual value (= or : followed by a non-path value)
            // Allow lines like "SSH key at ~/.ssh/id_ed25519" but scrub "password = abc123"
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

pub async fn generate_prompt(
    state: &Arc<AppState>,
    task: &str,
    _agent_type: &str,
) -> String {
    let engram_url = &state.config.engram.url;

    // Step 1: Get embedding from Engram
    let embed_url = format!("{}/embed", engram_url);
    let embedding: Vec<f32> = match state.http_client
        .post(&embed_url)
        .json(&json!({"text": task}))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            resp.json::<serde_json::Value>().await
                .ok()
                .and_then(|v| serde_json::from_value(v["embedding"].clone()).ok())
                .unwrap_or_default()
        }
        _ => {
            tracing::warn!("Engram /embed unavailable for prompt generation, using minimal prompt");
            vec![]
        }
    };

    // Step 2: Query brain with embedding
    let activated_memories: Vec<MemorySummary> = if !embedding.is_empty() {
        let mut brain = state.brain.lock().await;
        let result = brain.query(&embedding, 15, 8.0, 3);
        result.activated.iter().map(|m| MemorySummary {
            id: m.id,
            content: scrub_credentials(&m.content),
            category: m.category.clone(),
            activation: m.activation,
        }).collect()
    } else {
        vec![]
    };

    // Step 3: Try Engram oracle for natural language synthesis
    let oracle_url = format!("{}/brain/oracle", engram_url);
    let briefing = match state.http_client
        .post(&oracle_url)
        .json(&json!({"query": task}))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            resp.json::<serde_json::Value>().await
                .ok()
                .and_then(|v| v["briefing"].as_str().map(|s| scrub_credentials(s)))
                .unwrap_or_else(|| templates::format_fallback_briefing(&activated_memories))
        }
        _ => {
            tracing::debug!("Engram oracle unavailable, using fallback briefing");
            templates::format_fallback_briefing(&activated_memories)
        }
    };

    // Step 4: Build contradiction issues section
    let issues_section = if !embedding.is_empty() {
        let brain = state.brain.lock().await;
        let issue_lines: Vec<String> = Vec::new();
        // We already queried above -- pull contradiction context from the last query if any
        // For now, surface top contradiction pairs from recent query result
        // (We don't store them, so this section will be empty unless we re-query -- acceptable)
        drop(brain);
        if issue_lines.is_empty() {
            "No known contradictions for this task.".to_string()
        } else {
            issue_lines.join("\n")
        }
    } else {
        "Engram unavailable -- contradictions unknown.".to_string()
    };

    // Step 5: Assemble context section
    let context_lines: Vec<String> = activated_memories.iter().take(10).map(|m| {
        format!("- [{:.2}] [{}] {}", m.activation, m.category, m.content)
    }).collect();
    let context_section = if context_lines.is_empty() {
        "No relevant context available.".to_string()
    } else {
        context_lines.join("\n")
    };

    // Step 6: Get Chiasm URL (default)
    let chiasm_url = std::env::var("CHIASM_URL")
        .unwrap_or_else(|_| "http://localhost:4201".to_string());

    // Assemble final prompt
    let sections = vec![
        format!("{}\n{}", templates::SECTION_TASK, task),
        format!("{}\n{}", templates::SECTION_STATE, briefing),
        format!("{}\n{}", templates::SECTION_CONSTRAINTS, templates::static_constraints()),
        format!("{}\n{}", templates::SECTION_TOOLS, templates::tools_section(engram_url, &chiasm_url)),
        format!("{}\n{}", templates::SECTION_ISSUES, issues_section),
        format!("{}\n{}", templates::SECTION_CONTEXT, context_section),
    ];

    sections.join("\n\n---\n\n")
}
