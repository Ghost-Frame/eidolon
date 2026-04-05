use std::sync::Arc;
use crate::config::AgentsConfig;
use crate::llm::client::LlmClient;
use crate::syntheos::engram::EngramClient;
use crate::intelligence::budget::{ModelFamily, TokenBudget};
use crate::intelligence::compressor::{Compressor, CompressedContext};
use crate::intelligence::distiller::{Distiller, DistilledPrompt};
use crate::intelligence::router::RoutingDecision;
use crate::intelligence::selector::{Selector, ModelSelection};

/// Result of the full intelligence pipeline.
#[derive(Debug, Clone)]
pub struct PipelineResult {
    pub distilled: DistilledPrompt,
    pub selection: ModelSelection,
    pub compression: Option<CompressedContext>,
    pub budget: TokenBudget,
}

impl PipelineResult {
    /// Format for display to user before dispatch.
    pub fn format_for_approval(&self) -> String {
        let mut out = String::new();

        out.push_str(&format!("Agent: {} ({})\n", self.selection.agent, self.selection.model));
        out.push_str(&format!("Estimated cost: ${:.4}\n", self.selection.estimated_cost));
        out.push_str(&format!("Reasoning: {}\n\n", self.selection.reasoning));

        out.push_str("--- Distilled prompt ---\n");
        out.push_str(&self.distilled.format_for_display());

        if let Some(ref comp) = self.compression {
            out.push_str(&format!(
                "\n[Compression: {} -> {} tokens ({:.0}% reduction)]\n",
                comp.original_tokens,
                comp.compressed_tokens,
                (1.0 - comp.compression_ratio) * 100.0
            ));
        }

        out.push_str("\nSay yes to proceed, or tell me what to change.");
        out
    }
}

/// The intelligence pipeline. Owns the compressor (which has cache state)
/// and creates distiller/selector on demand.
pub struct Pipeline {
    compressor: Compressor,
    llm_client: Arc<LlmClient>,
    model_name: String,
}

impl Pipeline {
    pub fn new(llm_client: Arc<LlmClient>, model_name: &str) -> Self {
        Self {
            compressor: Compressor::new(llm_client.clone(), model_name),
            llm_client,
            model_name: model_name.to_string(),
        }
    }

    /// Run the full intelligence pipeline:
    /// 1. Compress conversation context (local LLM, free)
    /// 2. Search Engram for relevant memories
    /// 3. Distill user intent into structured task spec (local LLM, free)
    /// 4. Select optimal model based on complexity + budget
    pub async fn run(
        &mut self,
        user_msg: &str,
        history: &[(String, String)],
        decision: &RoutingDecision,
        agents_config: &AgentsConfig,
        engram_client: &Option<Arc<EngramClient>>,
    ) -> PipelineResult {
        // Step 1: Compress context
        let compression = self.compressor.compress(history, user_msg).await;
        let compressed_context = compression.context.clone();

        // Step 2: Search Engram for relevant context
        let engram_context = if let Some(ref engram) = engram_client {
            match engram.search(user_msg, 5).await {
                Ok(results) if !results.is_empty() => results.join("\n"),
                _ => String::new(),
            }
        } else {
            String::new()
        };

        // Step 3: Distill prompt
        let distiller = Distiller::new(self.llm_client.clone(), &self.model_name);
        let distilled = distiller.distill(user_msg, &compressed_context, &engram_context).await;

        // Step 4: Estimate budget and select model
        let agent_preference = decision.agent_needed.as_deref();
        let target_family = match agent_preference {
            Some("codex") => ModelFamily::OpenAI,
            _ => ModelFamily::Claude,
        };

        let full_prompt = distilled.format_for_agent();
        let context_limit = match target_family {
            ModelFamily::Claude => 200_000,
            ModelFamily::OpenAI => 128_000,
            ModelFamily::Qwen => 32_768,
        };
        let budget = TokenBudget::estimate(&full_prompt, &target_family, context_limit);

        let selection = Selector::select(
            &decision.complexity,
            &budget,
            agents_config,
            agent_preference,
        );

        let compression_result = if compression.compression_ratio < 1.0 {
            Some(compression)
        } else {
            None
        };

        PipelineResult {
            distilled,
            selection,
            compression: compression_result,
            budget,
        }
    }

    pub fn clear_cache(&mut self) {
        self.compressor.clear_cache();
    }
}
