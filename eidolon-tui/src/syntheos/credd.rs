use serde_json::json;

#[derive(Debug, Clone)]
pub struct CreddClient {
    base_url: String,
    agent_key: String,
}

impl CreddClient {
    pub fn new(base_url: &str, agent_key: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            agent_key: agent_key.to_string(),
        }
    }

    pub fn build_get_secret_url(&self, service: &str, key: &str) -> String {
        format!("{}/secret/{}/{}", self.base_url, service, key)
    }

    pub fn build_list_secrets_url(&self) -> String {
        format!("{}/secrets", self.base_url)
    }

    pub fn auth_header(&self) -> (&str, String) {
        ("Authorization", format!("Bearer {}", self.agent_key))
    }
}
