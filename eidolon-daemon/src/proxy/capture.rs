use std::sync::Arc;
use serde_json::json;
use super::classify::{classify_rule, classify_llm, extract_memory};
use super::session::ProxySessionTracker;
use super::Category;
use crate::AppState;

/// Run the capture pipeline asynchronously.
/// Called after the proxy response has been streamed back.
/// Processes new turns, classifies them, and stores worthy memories to Engram.
pub async fn run_capture(
    state: Arc<AppState>,
    tracker: Arc<ProxySessionTracker>,
    session_id: String,
    messages: Vec<serde_json::Value>,
) {
    if !state.config.proxy.capture.enabled {
        return;
    }

    let capture_config = &state.config.proxy.capture;

    // Check volume cap
    let stored = tracker.memories_stored(&session_id).await;
    if stored >= capture_config.max_memories_per_session {
        tracing::debug!("proxy capture: session {} hit memory cap ({})", session_id, stored);
        return;
    }

    // Get only new turns since last processing
    let new_turns = tracker.get_new_turns(&session_id, &messages).await;
    if new_turns.is_empty() {
        return;
    }

    tracing::debug!("proxy capture: session {} has {} new turns", session_id, new_turns.len());

    let _remaining_budget = capture_config.max_memories_per_session - stored;

    for turn in new_turns {
        // Only process user messages (not assistant responses)
        let role = turn.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if role != "user" {
            continue;
        }

        // Extract text content
        let content = extract_text_from_turn(&turn);
        if content.is_empty() {
            continue;
        }

        // Check volume cap
        let current_stored = tracker.memories_stored(&session_id).await;
        if current_stored >= capture_config.max_memories_per_session {
            break;
        }

        // Phase 1: Rule-based classification
        let category = match classify_rule(&content) {
            Some(Category::Skip) => continue,
            Some(cat) => cat,
            None => {
                // Phase 2: LLM classification for ambiguous cases
                let cat = classify_llm(
                    &state.http_client,
                    &capture_config.ollama_url,
                    &capture_config.classification_model,
                    &content,
                ).await;
                if cat == Category::Skip {
                    continue;
                }
                cat
            }
        };

        // Phase 3: Extract concise memory
        let memory_text = match extract_memory(
            &state.http_client,
            &capture_config.ollama_url,
            &capture_config.classification_model,
            &content,
        ).await {
            Some(text) => text,
            None => {
                // Fallback: use content directly (truncated)
                if content.len() > 500 {
                    content[..500].to_string()
                } else {
                    content.clone()
                }
            }
        };

        // Phase 4: Novelty check via embedding similarity
        if should_skip_duplicate(&state, &memory_text, capture_config.novelty_threshold).await {
            tracing::debug!("proxy capture: skipping duplicate memory");
            continue;
        }

        // Phase 5: Store to Engram
        if store_to_engram(&state, &memory_text, category, &session_id).await {
            tracker.increment_memories(&session_id).await;
            tracing::info!(
                "proxy capture: stored {} memory for session {} (total: {})",
                category.as_engram_category(),
                session_id,
                tracker.memories_stored(&session_id).await,
            );
        }
    }
}

/// Extract text content from a conversation turn (handles string and content block formats).
fn extract_text_from_turn(turn: &serde_json::Value) -> String {
    match turn.get("content") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(blocks)) => {
            blocks.iter()
                .filter_map(|b| {
                    if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                        b.get("text").and_then(|t| t.as_str()).map(|s| s.to_string())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        _ => String::new(),
    }
}

/// Check if a memory is too similar to recently stored memories.
/// Uses embedding similarity against brain's Hopfield network.
async fn should_skip_duplicate(
    state: &AppState,
    memory_text: &str,
    threshold: f32,
) -> bool {
    // Get embedding for the candidate memory
    let embedding = match state.embed_text(memory_text).await {
        Some(e) => e,
        None => return false, // Can't check, allow it
    };

    // Query the brain for similar patterns
    let mut brain = state.brain.lock().await;
    let result = brain.query(&embedding, 3, 8.0, 1);

    // If the top match has very high activation, it's a duplicate
    if let Some(top) = result.activated.first() {
        if top.activation > threshold {
            return true;
        }
    }

    false
}

/// Store a memory to Engram via HTTP.
async fn store_to_engram(
    state: &AppState,
    content: &str,
    category: Category,
    session_id: &str,
) -> bool {
    let engram_url = &state.config.engram.url;
    let url = format!("{}/store", engram_url);

    let payload = json!({
        "content": content,
        "category": category.as_engram_category(),
        "importance": 5,
        "source": "eidolon-proxy",
        "tags": ["auto-captured"],
        "session_id": session_id,
    });

    let mut req = state.http_client.post(&url)
        .json(&payload)
        .timeout(std::time::Duration::from_secs(5));

    if let Some(ref key) = state.config.engram.api_key {
        req = req.header("Authorization", format!("Bearer {}", key));
    }

    match req.send().await {
        Ok(resp) if resp.status().is_success() => true,
        Ok(resp) => {
            tracing::warn!("proxy capture: engram store failed: {}", resp.status());
            false
        }
        Err(e) => {
            tracing::warn!("proxy capture: engram store error: {}", e);
            false
        }
    }
}
