use serde_json::json;

#[derive(Debug, Clone)]
pub struct SomaClient {
    base_url: String,
    api_key: String,
}

impl SomaClient {
    pub fn new(base_url: &str, api_key: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
        }
    }

    pub fn build_register_request(&self, name: &str, agent_type: &str, description: &str, capabilities: &[&str]) -> (String, String) {
        let url = format!("{}/soma/agents", self.base_url);
        let body = json!({
            "name": name,
            "type": agent_type,
            "description": description,
            "capabilities": capabilities
        }).to_string();
        (url, body)
    }

    pub fn build_heartbeat_request(&self, agent_id: u64, status: &str) -> (String, String) {
        let url = format!("{}/soma/agents/{}/heartbeat", self.base_url, agent_id);
        let body = json!({"status": status}).to_string();
        (url, body)
    }
}
