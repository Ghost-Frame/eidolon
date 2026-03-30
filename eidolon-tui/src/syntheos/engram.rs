use serde_json::json;

#[derive(Debug, Clone)]
pub struct EngramClient {
    base_url: String,
    api_key: String,
}

impl EngramClient {
    pub fn new(base_url: &str, api_key: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
        }
    }

    pub fn build_search_request(&self, query: &str, limit: u32) -> (String, String) {
        let url = format!("{}/search", self.base_url);
        let body = json!({
            "query": query,
            "limit": limit
        }).to_string();
        (url, body)
    }

    pub fn build_store_request(&self, content: &str, source: &str, category: &str) -> (String, String) {
        let url = format!("{}/store", self.base_url);
        let body = json!({
            "content": content,
            "source": source,
            "category": category
        }).to_string();
        (url, body)
    }

    pub fn build_recall_request(&self, limit: u32) -> (String, String) {
        let url = format!("{}/recall", self.base_url);
        let body = json!({"limit": limit}).to_string();
        (url, body)
    }

    pub fn build_context_request(&self, query: &str) -> (String, String) {
        let url = format!("{}/context", self.base_url);
        let body = json!({"query": query}).to_string();
        (url, body)
    }
}
