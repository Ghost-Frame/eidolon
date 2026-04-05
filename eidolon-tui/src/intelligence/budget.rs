/// Model family for token estimation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelFamily {
    Qwen,   // Local, chars / 3.2
    Claude, // Anthropic, chars / 3.8
    OpenAI, // GPT/Codex, chars / 3.5
}

impl ModelFamily {
    /// Detect model family from model ID string.
    pub fn from_model_id(model: &str) -> Self {
        let lower = model.to_lowercase();
        if lower.contains("claude") || lower.contains("haiku") || lower.contains("sonnet") || lower.contains("opus") {
            ModelFamily::Claude
        } else if lower.contains("gpt") || lower.contains("codex") {
            ModelFamily::OpenAI
        } else {
            ModelFamily::Qwen
        }
    }

    /// Chars-per-token ratio. Conservative (overestimates tokens).
    fn chars_per_token(&self) -> f64 {
        match self {
            ModelFamily::Qwen => 3.2,
            ModelFamily::Claude => 3.8,
            ModelFamily::OpenAI => 3.5,
        }
    }
}

/// Token budget for a model call.
#[derive(Debug, Clone)]
pub struct TokenBudget {
    pub context_limit: u32,
    pub estimated_input: u32,
    pub reserved_output: u32,
    pub available: u32,
    pub estimated_cost_usd: f64,
}

/// Cost per million tokens (input) by model family.
/// Conservative estimates for budget planning.
fn cost_per_million_input(family: &ModelFamily) -> f64 {
    match family {
        ModelFamily::Qwen => 0.0,    // Local, free
        ModelFamily::Claude => 15.0, // Opus-tier pricing
        ModelFamily::OpenAI => 6.0,  // GPT-5.4 tier
    }
}

impl TokenBudget {
    /// Estimate token budget for a given text and target model.
    pub fn estimate(text: &str, family: &ModelFamily, context_limit: u32) -> Self {
        let estimated_input = (text.len() as f64 / family.chars_per_token()) as u32;
        let reserved_output = 4096.min(context_limit / 4);
        let available = context_limit.saturating_sub(estimated_input).saturating_sub(reserved_output);
        let cost_per_token = cost_per_million_input(family) / 1_000_000.0;
        let estimated_cost_usd = estimated_input as f64 * cost_per_token;

        Self {
            context_limit,
            estimated_input,
            reserved_output,
            available,
            estimated_cost_usd,
        }
    }

    /// Quick token estimate for a string.
    pub fn estimate_tokens(text: &str, family: &ModelFamily) -> u32 {
        (text.len() as f64 / family.chars_per_token()) as u32
    }

    /// Check if the input fits within the context window.
    pub fn fits(&self) -> bool {
        self.estimated_input + self.reserved_output <= self.context_limit
    }

    /// Returns true if compression is recommended (>70% of context used).
    pub fn needs_compression(&self) -> bool {
        let used_ratio = self.estimated_input as f64 / self.context_limit as f64;
        used_ratio > 0.7
    }
}
