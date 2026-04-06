use std::sync::Arc;
use crate::llm::client::LlmClient;

/// Cached summary of older conversation history.
#[derive(Debug, Clone)]
pub struct CachedSummary {
    pub summary: String,
    pub covers_up_to_index: usize,
    pub token_estimate: u32,
}

/// Result of context compression.
#[derive(Debug, Clone)]
pub struct CompressedContext {
    pub context: String,
    pub original_tokens: u32,
    pub compressed_tokens: u32,
    pub compression_ratio: f64,
}

pub struct Compressor {
    llm_client: Arc<LlmClient>,
    model_name: String,
    summary_cache: Option<CachedSummary>,
}

impl Compressor {
    pub fn new(llm_client: Arc<LlmClient>, model_name: &str) -> Self {
        Self {
            llm_client,
            model_name: model_name.to_string(),
            summary_cache: None,
        }
    }

    /// Compress conversation history for dispatch to a paid agent.
    /// Uses local LLM (free) to summarize older messages.
    pub async fn compress(
        &mut self,
        messages: &[(String, String)],
        current_task: &str,
    ) -> CompressedContext {
        let non_system: Vec<&(String, String)> = messages.iter()
            .filter(|(role, _)| role != "system")
            .collect();

        let original_text: String = non_system.iter()
            .map(|(_, content)| content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let original_tokens = (original_text.len() as f64 / 3.2) as u32;

        if non_system.len() <= 6 {
            return CompressedContext {
                context: original_text,
                original_tokens,
                compressed_tokens: original_tokens,
                compression_ratio: 1.0,
            };
        }

        let keep_recent = 4;
        let split_point = non_system.len() - keep_recent;

        let needs_new_summary = match &self.summary_cache {
            Some(cache) => cache.covers_up_to_index < split_point,
            None => true,
        };

        let summary = if needs_new_summary {
            let old_messages: Vec<String> = non_system[..split_point].iter()
                .map(|(role, content)| format!("[{}] {}", role, content))
                .collect();
            let old_text = old_messages.join("\n---\n");

            match self.summarize_via_llm(&old_text, current_task).await {
                Ok(s) => {
                    if s.len() >= old_text.len() {
                        old_text
                    } else {
                        let token_est = (s.len() as f64 / 3.2) as u32;
                        self.summary_cache = Some(CachedSummary {
                            summary: s.clone(),
                            covers_up_to_index: split_point,
                            token_estimate: token_est,
                        });
                        s
                    }
                }
                Err(_) => old_messages.join("\n"),
            }
        } else {
            self.summary_cache.as_ref()
                .map(|c| c.summary.clone())
                .unwrap_or_default()
        };

        let recent: Vec<String> = non_system[split_point..].iter()
            .map(|(role, content)| format!("[{}] {}", role, content))
            .collect();

        let compressed = format!(
            "[Conversation summary]\n{}\n\n[Recent messages]\n{}",
            summary,
            recent.join("\n---\n")
        );

        let compressed_tokens = (compressed.len() as f64 / 3.2) as u32;
        let compression_ratio = if original_tokens > 0 {
            compressed_tokens as f64 / original_tokens as f64
        } else {
            1.0
        };

        CompressedContext {
            context: compressed,
            original_tokens,
            compressed_tokens,
            compression_ratio,
        }
    }

    async fn summarize_via_llm(&self, old_text: &str, current_task: &str) -> Result<String, String> {
        let system = "You are a conversation compressor. Produce a dense factual summary. \
            Preserve: key decisions, file paths, variable names, error messages, technical details. \
            Drop: greetings, filler, repeated information, superseded messages. \
            Output only the summary.";

        let user_prompt = format!(
            "Current task: {}\n\nConversation to summarize:\n{}",
            current_task, old_text
        );

        let msgs: &[(&str, &str)] = &[
            ("system", system),
            ("user", &user_prompt),
        ];
        let mut request = LlmClient::build_request_with_model(
            &self.model_name, msgs, 0.3, None,
        );
        request.max_tokens = Some(1024);

        let resp = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            self.llm_client.complete(&request),
        )
        .await
        .map_err(|_| "Compression LLM call timed out".to_string())?
        .map_err(|e| format!("Compression LLM call failed: {}", e))?;

        resp.choices.first()
            .map(|c| c.message.content.trim().to_string())
            .ok_or_else(|| "Empty compression response".to_string())
    }

    pub fn clear_cache(&mut self) {
        self.summary_cache = None;
    }
}
