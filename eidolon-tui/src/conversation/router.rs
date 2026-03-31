use serde::Deserialize;
use crate::llm::client::LlmClient;
use crate::llm::grammar::intent_routing_grammar;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Intent {
    Casual,
    Memory,
    Action,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Complexity {
    Light,
    Medium,
    Heavy,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RoutingDecision {
    pub intent: Intent,
    pub confidence: f64,
    pub complexity: Complexity,
    pub tools_needed: Vec<String>,
    pub agent_needed: Option<String>,
    pub reasoning: String,
}

impl RoutingDecision {
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    pub fn needs_agent(&self) -> bool {
        self.agent_needed.is_some()
    }

    pub fn needs_tools(&self) -> bool {
        !self.tools_needed.is_empty()
    }

    /// Map complexity tier to a model ID based on which agent was selected.
    pub fn select_model(&self, config: &crate::config::AgentsConfig) -> String {
        let agent = self.agent_needed.as_deref().unwrap_or("claude");
        let entry = match agent {
            "codex" => &config.codex,
            _ => &config.claude,
        };
        match self.complexity {
            Complexity::Light => entry.model_light.clone(),
            Complexity::Medium => entry.model_medium.clone(),
            Complexity::Heavy => entry.model_heavy.clone(),
        }
    }

    /// Classify the user message by making a non-streaming LLM call with grammar constraints.
    pub async fn route(
        client: &LlmClient,
        user_message: &str,
        model_name: &str,
        temperature: f32,
    ) -> Result<Self, String> {
        let grammar = intent_routing_grammar();
        let system = r#"You are a routing system. Classify the user's intent and task complexity. Output JSON only.

Complexity guide:
- "light": simple tasks -- write a small file, fix a typo, rename something, quick lookup, one-file changes
- "medium": moderate tasks -- implement a feature, debug an issue, refactor a module, multi-file edits
- "heavy": complex tasks -- architectural changes, multi-system work, security-sensitive operations, large refactors"#;
        let msgs: &[(&str, &str)] = &[
            ("system", system),
            ("user", user_message),
        ];
        let mut request = LlmClient::build_request_with_model(
            model_name,
            msgs,
            temperature,
            Some(&grammar),
        );
        request.max_tokens = Some(150);

        let resp = client.complete(&request).await
            .map_err(|e| format!("Routing LLM call failed: {}", e))?;

        let content = resp.choices.first()
            .map(|c| c.message.content.trim().to_string())
            .ok_or_else(|| "Empty routing response".to_string())?;

        RoutingDecision::from_json(&content)
            .map_err(|e| format!("Failed to parse routing decision '{}': {}", content, e))
    }
}
