use reqwest::Client;
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct EngramClient {
    http: Client,
    base_url: String,
    api_key: String,
}

impl EngramClient {
    pub fn new(base_url: &str, api_key: &str) -> Self {
        Self {
            http: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
        }
    }

    fn auth(&self) -> String {
        format!("Bearer {}", self.api_key)
    }

    pub async fn search(&self, query: &str, limit: u32) -> Result<Vec<String>, String> {
        let url = format!("{}/search", self.base_url);
        let resp = self.http
            .post(&url)
            .header("Authorization", self.auth())
            .json(&json!({"query": query, "limit": limit}))
            .send()
            .await
            .map_err(|e| format!("Engram search failed: {}", e))?;

        let body: Value = resp.json().await
            .map_err(|e| format!("Engram search parse error: {}", e))?;

        let results = body.as_array()
            .map(|arr| arr.iter()
                .filter_map(|v| v.get("content").and_then(|c| c.as_str()).map(|s| s.to_string()))
                .collect())
            .unwrap_or_default();

        Ok(results)
    }

    pub async fn store(&self, content: &str, source: &str, category: &str) -> Result<(), String> {
        let url = format!("{}/store", self.base_url);
        self.http
            .post(&url)
            .header("Authorization", self.auth())
            .json(&json!({"content": content, "source": source, "category": category}))
            .send()
            .await
            .map_err(|e| format!("Engram store failed: {}", e))?;
        Ok(())
    }

    /// Legacy URL-builder shims used by prompt_generator.rs
    pub fn build_search_request(&self, query: &str, limit: u32) -> (String, String) {
        let url = format!("{}/search", self.base_url);
        let body = serde_json::json!({"query": query, "limit": limit}).to_string();
        (url, body)
    }

    pub fn build_store_request(&self, content: &str, source: &str, category: &str) -> (String, String) {
        let url = format!("{}/store", self.base_url);
        let body = serde_json::json!({"content": content, "source": source, "category": category}).to_string();
        (url, body)
    }

    pub fn auth_header(&self) -> String {
        self.auth()
    }

    pub async fn context(&self, query: &str) -> Result<String, String> {
        let url = format!("{}/context", self.base_url);
        let resp = self.http
            .post(&url)
            .header("Authorization", self.auth())
            .json(&json!({"query": query}))
            .send()
            .await
            .map_err(|e| format!("Engram context failed: {}", e))?;

        let body: Value = resp.json().await
            .map_err(|e| format!("Engram context parse error: {}", e))?;

        Ok(body.get("context")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string())
    }
}
