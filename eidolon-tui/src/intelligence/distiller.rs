use std::sync::Arc;
use serde::Deserialize;
use crate::llm::client::LlmClient;

/// Structured task specification produced by distilling user intent.
#[derive(Debug, Clone, Deserialize)]
pub struct DistilledPrompt {
    pub objective: String,
    pub constraints: Vec<String>,
    pub context: String,
    pub expected_output: String,
}

impl DistilledPrompt {
    /// Format for display to the user before dispatch.
    pub fn format_for_display(&self) -> String {
        let mut out = format!("Objective: {}\n", self.objective);
        if !self.constraints.is_empty() {
            out.push_str("Constraints:\n");
            for c in &self.constraints {
                out.push_str(&format!("  - {}\n", c));
            }
        }
        if !self.context.is_empty() {
            out.push_str(&format!("Context: {}\n", self.context));
        }
        if !self.expected_output.is_empty() {
            out.push_str(&format!("Expected output: {}\n", self.expected_output));
        }
        out
    }

    /// Format as a prompt string for the agent.
    pub fn format_for_agent(&self) -> String {
        let mut out = format!("## Task\n{}\n", self.objective);
        if !self.constraints.is_empty() {
            out.push_str("\n## Constraints\n");
            for c in &self.constraints {
                out.push_str(&format!("- {}\n", c));
            }
        }
        if !self.context.is_empty() {
            out.push_str(&format!("\n## Context\n{}\n", self.context));
        }
        if !self.expected_output.is_empty() {
            out.push_str(&format!("\n## Expected Output\n{}\n", self.expected_output));
        }
        out
    }

    /// Create a passthrough distilled prompt from raw user message (fallback).
    pub fn passthrough(user_msg: &str) -> Self {
        Self {
            objective: user_msg.to_string(),
            constraints: vec![],
            context: String::new(),
            expected_output: String::new(),
        }
    }
}

pub struct Distiller {
    llm_client: Arc<LlmClient>,
    model_name: String,
}

impl Distiller {
    pub fn new(llm_client: Arc<LlmClient>, model_name: &str) -> Self {
        Self {
            llm_client,
            model_name: model_name.to_string(),
        }
    }

    /// Distill user intent into a structured task specification.
    pub async fn distill(
        &self,
        user_msg: &str,
        compressed_context: &str,
        engram_context: &str,
    ) -> DistilledPrompt {
        match self.distill_via_llm(user_msg, compressed_context, engram_context).await {
            Ok(prompt) => prompt,
            Err(_) => DistilledPrompt::passthrough(user_msg),
        }
    }

    async fn distill_via_llm(
        &self,
        user_msg: &str,
        compressed_context: &str,
        engram_context: &str,
    ) -> Result<DistilledPrompt, String> {
        let system = "You are a prompt distiller. Convert the user's message into a structured task specification. \
            Extract the core objective, constraints, relevant context, and expected output. \
            Be precise and technical. Drop filler words and ambiguity. \
            Respond with ONLY a JSON object, no other text.\n\n\
            Required JSON format:\n\
            {\"objective\": \"clear task description\", \"constraints\": [\"list of constraints\"], \"context\": \"relevant background\", \"expected_output\": \"what the result should look like\"}";

        let mut user_prompt = format!("User message: {}", user_msg);
        if !compressed_context.is_empty() {
            user_prompt.push_str(&format!("\n\nConversation context:\n{}", compressed_context));
        }
        if !engram_context.is_empty() {
            user_prompt.push_str(&format!("\n\nMemory context:\n{}", engram_context));
        }

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
        .map_err(|_| "Distillation timed out".to_string())?
        .map_err(|e| format!("Distillation LLM call failed: {}", e))?;

        let content = resp.choices.first()
            .map(|c| c.message.content.trim().to_string())
            .ok_or_else(|| "Empty distillation response".to_string())?;

        if let Ok(prompt) = serde_json::from_str::<DistilledPrompt>(&content) {
            return Ok(prompt);
        }

        if let Some(json) = extract_json_object(&content) {
            if let Ok(prompt) = serde_json::from_str::<DistilledPrompt>(&json) {
                return Ok(prompt);
            }
        }

        Err(format!("Failed to parse distilled prompt from: {}", content))
    }
}

fn extract_json_object(text: &str) -> Option<String> {
    super::json_repair::extract_and_repair_json(text)
        .map(|v| v.to_string())
}
