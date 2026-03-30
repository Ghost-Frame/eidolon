use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Intent {
    Casual,
    Memory,
    Action,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RoutingDecision {
    pub intent: Intent,
    pub confidence: f64,
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
}
