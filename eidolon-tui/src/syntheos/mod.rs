pub mod engram;
pub mod chiasm;
pub mod axon;
pub mod broca;
pub mod soma;
pub mod openspace;
pub mod credd;

use reqwest::Client;

/// Shared HTTP client wrapper for all Syntheos services.
/// All services share the same base URL and API key.
#[derive(Debug, Clone)]
pub struct SyntheosBase {
    pub base_url: String,
    pub api_key: String,
    pub http: Client,
}

impl SyntheosBase {
    pub fn new(base_url: &str, api_key: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            http: Client::new(),
        }
    }

    pub async fn post(&self, path: &str, body: &str) -> Result<String, reqwest::Error> {
        let resp = self.http
            .post(format!("{}{}", self.base_url, path))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send()
            .await?
            .text()
            .await?;
        Ok(resp)
    }

    pub async fn get(&self, path: &str) -> Result<String, reqwest::Error> {
        let resp = self.http
            .get(format!("{}{}", self.base_url, path))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?
            .text()
            .await?;
        Ok(resp)
    }
}
