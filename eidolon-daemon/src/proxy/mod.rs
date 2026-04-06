pub mod capture;
pub mod classify;
pub mod handler;
pub mod recall;
pub mod session;

use serde::{Deserialize, Serialize};

/// Memory category for classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Fact,
    Decision,
    Preference,
    StateChange,
    Discovery,
    Issue,
    Skip,
}

impl Category {
    pub fn as_engram_category(&self) -> &'static str {
        match self {
            Category::Fact => "reference",
            Category::Decision => "decision",
            Category::Preference => "discovery",
            Category::StateChange => "state",
            Category::Discovery => "discovery",
            Category::Issue => "issue",
            Category::Skip => "general",
        }
    }

    pub fn from_str_loose(s: &str) -> Self {
        let s = s.trim().to_lowercase();
        match s.as_str() {
            "fact" | "reference" => Category::Fact,
            "decision" => Category::Decision,
            "preference" | "pref" => Category::Preference,
            "state_change" | "state" | "statechange" => Category::StateChange,
            "discovery" | "discover" => Category::Discovery,
            "issue" | "bug" | "error" => Category::Issue,
            _ => Category::Skip,
        }
    }
}

/// Represents a message in the Anthropic conversation format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: String,
    pub content: serde_json::Value,
}

impl ConversationMessage {
    /// Extract text content from message, handling both string and content block formats
    pub fn text_content(&self) -> String {
        match &self.content {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Array(blocks) => {
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
}
