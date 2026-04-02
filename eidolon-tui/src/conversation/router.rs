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

    /// Try to extract a routing decision from potentially messy LLM output.
    /// Layer 1: direct JSON parse. Layer 2: extract JSON substring. Layer 3: keyword fallback.
    pub fn extract_from_text(text: &str) -> Result<Self, String> {
        let trimmed = text.trim();

        // Layer 1: direct parse (try as-is then lowercased for case-insensitive enums)
        if let Ok(decision) = serde_json::from_str::<Self>(trimmed) {
            return Ok(decision);
        }
        if let Ok(decision) = serde_json::from_str::<Self>(&trimmed.to_lowercase()) {
            return Ok(decision);
        }

        // Layer 2: extract JSON object from surrounding text
        if let Some(json) = Self::extract_json_substring(trimmed) {
            if let Ok(decision) = serde_json::from_str::<Self>(&json) {
                return Ok(decision);
            }
            if let Ok(decision) = serde_json::from_str::<Self>(&json.to_lowercase()) {
                return Ok(decision);
            }
        }

        // Layer 3: keyword-based fallback
        Ok(Self::keyword_fallback(trimmed))
    }

    /// Find the first top-level JSON object in the text by matching braces.
    fn extract_json_substring(text: &str) -> Option<String> {
        let start = text.find('{')?;
        let mut depth = 0i32;
        let mut end = None;
        for (i, ch) in text[start..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(start + i + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        end.map(|e| text[start..e].to_string())
    }

    /// Classify intent from plain text using keyword scanning.
    pub fn keyword_fallback(text: &str) -> Self {
        let lower = text.to_lowercase();

        let action_keywords = [
            "fix", "implement", "build", "deploy", "execute", "run",
            "refactor", "create", "delete", "install", "update",
            "debug", "write", "add feature", "spawn", "launch",
            "start a", "open a", "make a", "set up", "configure",
        ];
        let memory_keywords = [
            "remember", "recall", "search engram", "what was",
            "memory", "look up", "find in", "past decision",
        ];

        let action_matches: usize = action_keywords.iter()
            .filter(|kw| lower.contains(*kw))
            .count();
        let memory_matches: usize = memory_keywords.iter()
            .filter(|kw| lower.contains(*kw))
            .count();

        let (intent, match_count) = if action_matches > memory_matches && action_matches > 0 {
            (Intent::Action, action_matches)
        } else if memory_matches > 0 {
            (Intent::Memory, memory_matches)
        } else {
            (Intent::Casual, 0)
        };

        // Scale confidence by match count: 1 match = 0.35, 2 = 0.5, 3+ = 0.65
        let confidence = match match_count {
            0 => 0.2,
            1 => 0.35,
            2 => 0.5,
            _ => 0.65,
        };

        let agent_needed = if intent == Intent::Action {
            if lower.contains("codex") {
                Some("codex".to_string())
            } else {
                Some("claude".to_string())
            }
        } else {
            None
        };

        Self {
            intent,
            confidence,
            complexity: Complexity::Medium,
            tools_needed: vec![],
            agent_needed,
            reasoning: format!("Keyword fallback ({} matches)", match_count),
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

        let resp = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            client.complete(&request),
        )
        .await
        .map_err(|_| "Routing LLM call timed out after 15s".to_string())?
        .map_err(|e| format!("Routing LLM call failed: {}", e))?;

        let content = resp.choices.first()
            .map(|c| c.message.content.trim().to_string())
            .ok_or_else(|| "Empty routing response".to_string())?;

        Self::extract_from_text(&content)
    }
}
