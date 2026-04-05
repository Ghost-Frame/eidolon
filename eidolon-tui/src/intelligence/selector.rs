use crate::config::AgentsConfig;
use crate::intelligence::router::Complexity;
use crate::intelligence::budget::{ModelFamily, TokenBudget};

/// Result of adaptive model selection.
#[derive(Debug, Clone)]
pub struct ModelSelection {
    pub agent: String,
    pub model: String,
    pub reasoning: String,
    pub estimated_cost: f64,
}

pub struct Selector;

impl Selector {
    /// Select the optimal agent and model based on task characteristics.
    pub fn select(
        complexity: &Complexity,
        budget: &TokenBudget,
        agents_config: &AgentsConfig,
        preferred_agent: Option<&str>,
    ) -> ModelSelection {
        let agent = preferred_agent.unwrap_or("claude");

        let entry = match agent {
            "codex" => &agents_config.codex,
            _ => &agents_config.claude,
        };

        // Base model from complexity tier
        let (base_model, tier_reason) = match complexity {
            Complexity::Light => (&entry.model_light, "light complexity"),
            Complexity::Medium => (&entry.model_medium, "medium complexity"),
            Complexity::Heavy => (&entry.model_heavy, "heavy complexity"),
        };

        // Upgrade for large context if needed
        let (model, context_reason) = if budget.estimated_input > 50_000 {
            (&entry.model_heavy, Some("large context (>50k tokens)"))
        } else if budget.estimated_input > 20_000 && matches!(complexity, Complexity::Light) {
            (&entry.model_medium, Some("medium context (>20k tokens), upgrading from light"))
        } else {
            (base_model, None)
        };

        // Estimate cost for selected model
        let family = ModelFamily::from_model_id(model);
        let estimated_cost = budget.estimated_input as f64
            * match family {
                ModelFamily::Qwen => 0.0,
                ModelFamily::Claude => 15.0 / 1_000_000.0,
                ModelFamily::OpenAI => 6.0 / 1_000_000.0,
            };

        let mut reasoning = format!("{} ({})", agent, tier_reason);
        if let Some(r) = context_reason {
            reasoning.push_str(&format!(". Adjusted: {}", r));
        }

        ModelSelection {
            agent: agent.to_string(),
            model: model.clone(),
            reasoning,
            estimated_cost,
        }
    }
}
