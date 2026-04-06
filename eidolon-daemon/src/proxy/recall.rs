use std::sync::Arc;
use std::time::Instant;
use super::session::ProxySessionTracker;
use crate::AppState;

/// Run the recall pipeline. Queries Engram, filters via Hopfield, synthesizes context.
/// Returns the injected context string if injection happened, None if skipped.
/// Modifies the request body in-place to inject context into the system prompt.
pub async fn run_recall(
    state: Arc<AppState>,
    tracker: Arc<ProxySessionTracker>,
    body: &mut serde_json::Value,
    session_id: &str,
) -> Option<String> {
    if !state.config.proxy.recall.enabled {
        return None;
    }

    let start = Instant::now();
    let recall_config = &state.config.proxy.recall;

    // Extract the latest user message as the query
    let query = extract_latest_user_message(body)?;
    if query.is_empty() {
        return None;
    }

    // Step 1: Query Engram for relevant context
    let engram_context = query_engram(
        &state,
        &query,
        recall_config.max_tokens,
        &recall_config.engram_mode,
    ).await;

    let engram_text = match engram_context {
        Some(text) if !text.is_empty() => text,
        _ => return None,
    };

    // Step 2: Hopfield filter -- refine with associative resonance
    let filtered = hopfield_filter(&state, &query, &engram_text).await;
    let context_text = if filtered.is_empty() {
        engram_text
    } else {
        filtered
    };

    // Step 3: Synthesize into a tight block
    let synthesized = synthesize_context(&context_text, recall_config.max_tokens);

    // Step 4: Differential check -- skip if too similar to last injection
    if tracker.check_staleness(session_id, &synthesized, recall_config.staleness_threshold).await {
        tracing::debug!(
            "proxy recall: skipping stale injection for session {} ({}ms)",
            session_id,
            start.elapsed().as_millis()
        );
        return None;
    }

    // Step 5: Inject into system prompt
    inject_into_system(body, &synthesized);
    tracker.update_last_injection(session_id, synthesized.clone()).await;

    let elapsed = start.elapsed();
    tracing::info!(
        "proxy recall: injected {} chars for session {} ({}ms)",
        synthesized.len(),
        session_id,
        elapsed.as_millis()
    );

    Some(synthesized)
}

/// Extract the latest user message text from the request body.
fn extract_latest_user_message(body: &serde_json::Value) -> Option<String> {
    let messages = body.get("messages")?.as_array()?;

    // Find the last user message
    for msg in messages.iter().rev() {
        if msg.get("role").and_then(|r| r.as_str()) == Some("user") {
            return Some(extract_text_content(msg));
        }
    }

    None
}

/// Extract text content from a message (handles string and content blocks).
fn extract_text_content(msg: &serde_json::Value) -> String {
    match msg.get("content") {
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

/// Query Engram's /context endpoint for relevant memories.
async fn query_engram(
    state: &AppState,
    query: &str,
    max_tokens: usize,
    mode: &str,
) -> Option<String> {
    let engram_url = &state.config.engram.url;
    let url = format!("{}/context", engram_url);

    let payload = serde_json::json!({
        "query": query,
        "max_tokens": max_tokens,
        "mode": mode,
    });

    let mut req = state.http_client.post(&url)
        .json(&payload)
        .timeout(std::time::Duration::from_millis(150)); // Tight timeout for recall budget

    if let Some(ref key) = state.config.engram.api_key {
        req = req.header("Authorization", format!("Bearer {}", key));
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("proxy recall: engram /context failed: {}", e);
            return None;
        }
    };

    if !resp.status().is_success() {
        tracing::warn!("proxy recall: engram /context returned {}", resp.status());
        return None;
    }

    let body: serde_json::Value = resp.json().await.ok()?;

    // Engram /context returns a structured response with context text
    // Try "context" field first, then "text", then stringify the whole thing
    body.get("context")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            body.get("text")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
}

/// Filter Engram results through Eidolon's Hopfield network.
/// Returns the most resonant memories as a synthesized string.
async fn hopfield_filter(
    state: &AppState,
    query: &str,
    _engram_text: &str,
) -> String {
    // Embed the query for Hopfield lookup
    let embedding = match state.embed_text(query).await {
        Some(e) => e,
        None => return String::new(),
    };

    // Query the Hopfield network
    let mut brain = state.brain.lock().await;
    let result = brain.query(&embedding, 5, 8.0, 2);
    drop(brain); // Release lock ASAP

    if result.activated.is_empty() {
        return String::new();
    }

    // Collect high-activation memories (>0.3 threshold)
    let relevant: Vec<&str> = result.activated.iter()
        .filter(|m| m.activation > 0.3)
        .map(|m| m.content.as_str())
        .collect();

    if relevant.is_empty() {
        return String::new();
    }

    // Combine Engram context with Hopfield-validated memories
    // Prefer Hopfield memories as they represent associative resonance
    let mut combined = String::new();
    for mem in relevant.iter().take(3) {
        if !combined.is_empty() {
            combined.push(' ');
        }
        combined.push_str(mem);
    }

    combined
}

/// Synthesize context into a tight block, respecting token budget.
/// Rough estimate: 1 token ~ 4 chars.
fn synthesize_context(context: &str, max_tokens: usize) -> String {
    let max_chars = max_tokens * 4;

    if context.len() <= max_chars {
        context.to_string()
    } else {
        // Truncate at a sentence boundary if possible
        let truncated = &context[..max_chars];
        if let Some(pos) = truncated.rfind(". ") {
            truncated[..=pos].to_string()
        } else {
            format!("{}...", truncated)
        }
    }
}

/// Inject context into the request's system prompt.
fn inject_into_system(body: &mut serde_json::Value, context: &str) {
    let block = format!("<engram-context>\n{}\n</engram-context>", context);

    // Anthropic Messages API: "system" can be a string or array of content blocks
    match body.get_mut("system") {
        Some(serde_json::Value::String(existing)) => {
            // Prepend to existing system string
            let new_system = format!("{}\n\n{}", block, existing);
            *existing = new_system;
        }
        Some(serde_json::Value::Array(blocks)) => {
            // Prepend as a text content block
            let context_block = serde_json::json!({
                "type": "text",
                "text": block,
            });
            blocks.insert(0, context_block);
        }
        Some(_) => {
            // Unexpected format, replace with string
            body["system"] = serde_json::Value::String(block);
        }
        None => {
            // No system prompt exists, create one
            body["system"] = serde_json::Value::String(block);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_latest_user_message() {
        let body = serde_json::json!({
            "model": "claude-opus-4-6",
            "messages": [
                {"role": "user", "content": "hello"},
                {"role": "assistant", "content": "hi there"},
                {"role": "user", "content": "what's the weather?"},
            ]
        });

        let msg = extract_latest_user_message(&body);
        assert_eq!(msg, Some("what's the weather?".to_string()));
    }

    #[test]
    fn test_extract_content_blocks() {
        let body = serde_json::json!({
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "look at this"},
                    {"type": "image", "source": {}},
                    {"type": "text", "text": "what do you see?"}
                ]},
            ]
        });

        let msg = extract_latest_user_message(&body);
        assert_eq!(msg, Some("look at this\nwhat do you see?".to_string()));
    }

    #[test]
    fn test_inject_into_system_string() {
        let mut body = serde_json::json!({
            "system": "You are a helpful assistant.",
            "messages": []
        });

        inject_into_system(&mut body, "User prefers dark mode.");

        let system = body.get("system").unwrap().as_str().unwrap();
        assert!(system.starts_with("<engram-context>"));
        assert!(system.contains("User prefers dark mode."));
        assert!(system.contains("You are a helpful assistant."));
    }

    #[test]
    fn test_inject_into_system_array() {
        let mut body = serde_json::json!({
            "system": [
                {"type": "text", "text": "You are a helpful assistant."}
            ],
            "messages": []
        });

        inject_into_system(&mut body, "User prefers dark mode.");

        let system = body.get("system").unwrap().as_array().unwrap();
        assert_eq!(system.len(), 2);
        assert!(system[0].get("text").unwrap().as_str().unwrap().contains("engram-context"));
    }

    #[test]
    fn test_inject_into_missing_system() {
        let mut body = serde_json::json!({
            "messages": [{"role": "user", "content": "hi"}]
        });

        inject_into_system(&mut body, "User prefers dark mode.");

        let system = body.get("system").unwrap().as_str().unwrap();
        assert!(system.contains("engram-context"));
    }

    #[test]
    fn test_synthesize_truncation() {
        let long = "a ".repeat(2000); // 4000 chars
        let result = synthesize_context(&long, 500); // 2000 char budget
        // Truncated + "..." = at most 2003 chars
        assert!(result.len() <= 2004, "result was {} chars", result.len());
    }
}
