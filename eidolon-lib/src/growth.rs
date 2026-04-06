//! Growth engine -- observe -> reflect -> record -> inject
//!
//! Provides probabilistic reflection after dream cycles and other events.
//! LLM access via Together.ai HTTP API.

use serde::{Deserialize, Serialize};

/// Growth configuration (from TOML)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrowthConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_reflection_chance")]
    pub reflection_chance: f64,
    #[serde(default = "default_llm_url")]
    pub llm_url: String,
    #[serde(default = "default_llm_model")]
    pub llm_model: String,
    /// Loaded at runtime from credd, not from config file
    #[serde(skip)]
    pub llm_api_key: Option<String>,
}

fn default_enabled() -> bool { true }
fn default_reflection_chance() -> f64 { 0.20 }
fn default_llm_url() -> String { "https://api.together.xyz/v1/chat/completions".to_string() }
fn default_llm_model() -> String { "meta-llama/Llama-3.3-70B-Instruct-Turbo".to_string() }

impl Default for GrowthConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            reflection_chance: default_reflection_chance(),
            llm_url: default_llm_url(),
            llm_model: default_llm_model(),
            llm_api_key: None,
        }
    }
}

/// Check if reflection should trigger (probability gate)
pub fn should_reflect(config: &GrowthConfig) -> bool {
    if !config.enabled { return false; }
    rand::random::<f64>() < config.reflection_chance
}

/// Validate an observation returned by the LLM
pub fn validate_observation(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.len() < 10 || trimmed.len() > 500 { return false; }
    if trimmed.eq_ignore_ascii_case("nothing") { return false; }
    if trimmed.starts_with("I don't") || trimmed.starts_with("There is nothing") { return false; }
    true
}

/// Build context string from dream cycle results
pub fn build_dream_context(
    replayed: usize,
    merged: usize,
    pruned_patterns: usize,
    pruned_edges: usize,
    discovered: usize,
    decorrelated: usize,
    resolved: usize,
    cycle_time_ms: u64,
    pattern_count: usize,
    edge_count: usize,
) -> Vec<String> {
    vec![
        format!("Dream cycle completed in {}ms", cycle_time_ms),
        format!("Replayed {} recent patterns, merged {} redundant", replayed, merged),
        format!("Pruned {} dead patterns and {} weak edges", pruned_patterns, pruned_edges),
        format!("Discovered {} new connections, decorrelated {}", discovered, decorrelated),
        format!("Resolved {} contradictions", resolved),
        format!("Current substrate: {} patterns, {} edges", pattern_count, edge_count),
    ]
}

/// Request body for /growth/reflect
#[derive(Debug, Deserialize)]
pub struct ReflectRequest {
    pub service: String,
    pub context: Vec<String>,
    pub existing_growth: Option<String>,
    pub prompt_override: Option<String>,
}

/// Response for /growth/reflect
#[derive(Debug, Serialize)]
pub struct ReflectResponse {
    pub observation: Option<String>,
}

/// Together.ai chat completion request
#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f64,
    max_tokens: u32,
}

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Option<Vec<ChatChoice>>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: Option<ChatChoiceMessage>,
}

#[derive(Deserialize)]
struct ChatChoiceMessage {
    content: Option<String>,
}

/// Domain-specific system prompts
fn get_system_prompt(service: &str, prompt_override: Option<&str>) -> String {
    if let Some(ovr) = prompt_override {
        return ovr.to_string();
    }
    let base = match service {
        "eidolon" => "You are Eidolon's self-reflection process. Eidolon is the daemon that orchestrates the Syntheos neurosymbolic brain.\n\nExamine the dream cycle results and ask yourself:\n- What did this dream cycle reveal about memory patterns?\n- Which patterns keep merging (over-correlated)?\n- What connections are surprising?\n- Is the substrate getting better or worse at targeted activation?",
        "claude-code" => "You are the self-reflection process for Claude Code sessions with Master (Zan).\n\nExamine the session activity and ask yourself:\n- Did a particular approach to a task work well or poorly?\n- Did Master correct a pattern that should be remembered?\n- Was there drift from expected behavior? Why?\n- Was something learned about the codebase or infrastructure?\n- Was there a communication style Master preferred?",
        "engram" => "You are Engram's internal self-reflection process. Engram is a persistent memory system for the Syntheos ecosystem.\n\nExamine the recent activity and ask yourself:\n- Which memories get searched most vs never?\n- What contradictions persist unresolved?\n- What knowledge gaps exist?\n- What categories are growing fastest?",
        "chiasm" => "You are Chiasm's self-reflection process. Chiasm is the task management system.\n\nExamine recent task completions and ask yourself:\n- What task patterns are emerging?\n- How accurate are time estimates?\n- Which agents are most reliable?\n- What recurring blockers should be addressed?",
        "thymus" => "You are Thymus's self-reflection process. Thymus monitors agent compliance and quality.\n\nExamine recent quality reports and ask yourself:\n- Which compliance rules drift most frequently?\n- Which agents show improvement over time?\n- Are there patterns in when drift occurs?\n- What quality signals are most predictive?",
        _ => "You are a self-reflection process for a Syntheos service.\n\nExamine the recent activity and extract ONE useful observation.",
    };

    format!("{}\n\nRules:\n- Output ONE concise observation (1-3 sentences max)\n- Write in first person as {}\n- Be specific -- not generic advice\n- If nothing interesting happened, output exactly: NOTHING\n- Do NOT output meta-commentary, explanations, or multiple options\n- Do NOT repeat things already known", base, service)
}

/// Call Together.ai (or compatible) LLM for reflection
pub async fn reflect(
    client: &reqwest::Client,
    config: &GrowthConfig,
    service: &str,
    context: &[String],
    existing_growth: Option<&str>,
    prompt_override: Option<&str>,
) -> Result<Option<String>, String> {
    let api_key = config.llm_api_key.as_deref()
        .ok_or_else(|| "growth LLM API key not configured".to_string())?;

    let system_prompt = get_system_prompt(service, prompt_override);

    let mut user_content = format!("Recent activity:\n\n{}\n\n", context.join("\n"));
    if let Some(existing) = existing_growth {
        let truncated = if existing.len() > 4000 { &existing[..4000] } else { existing };
        user_content.push_str(&format!("Things I already know (do NOT repeat these):\n{}\n\n", truncated));
    }
    user_content.push_str("What did I learn or notice? One observation, or NOTHING.");

    let req = ChatRequest {
        model: config.llm_model.clone(),
        messages: vec![
            ChatMessage { role: "system".to_string(), content: system_prompt },
            ChatMessage { role: "user".to_string(), content: user_content },
        ],
        temperature: 0.7,
        max_tokens: 300,
    };

    let resp = client.post(&config.llm_url)
        .bearer_auth(api_key)
        .json(&req)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("LLM request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("LLM returned {}: {}", status, &body[..body.len().min(200)]));
    }

    let chat_resp: ChatResponse = resp.json().await
        .map_err(|e| format!("Failed to parse LLM response: {}", e))?;

    let text = chat_resp.choices
        .and_then(|c| c.into_iter().next())
        .and_then(|c| c.message)
        .and_then(|m| m.content)
        .unwrap_or_default();

    let trimmed = text.trim().to_string();

    if validate_observation(&trimmed) {
        Ok(Some(trimmed))
    } else {
        Ok(None)
    }
}
