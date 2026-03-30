use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct BrocaClient {
    base_url: String,
    api_key: String,
}

impl BrocaClient {
    pub fn new(base_url: &str, api_key: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
        }
    }

    pub fn build_ask_request(&self, question: &str) -> (String, String) {
        let url = format!("{}/broca/ask", self.base_url);
        let body = json!({"question": question}).to_string();
        (url, body)
    }

    pub fn build_log_action_request(&self, agent: &str, service: &str, action: &str, payload: &Value) -> (String, String) {
        let url = format!("{}/broca/actions", self.base_url);
        let body = json!({
            "agent": agent,
            "service": service,
            "action": action,
            "payload": payload
        }).to_string();
        (url, body)
    }

    pub fn build_feed_url(&self, limit: u32) -> String {
        format!("{}/broca/feed?limit={}", self.base_url, limit)
    }
}
