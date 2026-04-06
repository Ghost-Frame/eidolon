use super::{AsyncEmbedError, AsyncEmbeddingProvider};

/// Embedding provider that calls Engram's /embed endpoint.
pub struct EngramProvider {
    http: reqwest::Client,
    url: String,
    api_key: Option<String>,
    dim: usize,
}

impl EngramProvider {
    pub fn new(http: reqwest::Client, url: String, api_key: Option<String>, dim: usize) -> Self {
        EngramProvider { http, url, api_key, dim }
    }
}

#[async_trait::async_trait]
impl AsyncEmbeddingProvider for EngramProvider {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, AsyncEmbedError> {
        let url = format!("{}/embed", self.url);
        let mut req = self.http
            .post(&url)
            .json(&serde_json::json!({"text": text}));
        if let Some(ref key) = self.api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }

        let resp = req.send().await
            .map_err(|e| AsyncEmbedError::NetworkError(format!("engram /embed: {}", e)))?;

        if !resp.status().is_success() {
            return Err(AsyncEmbedError::ProviderError(
                format!("engram /embed returned {}", resp.status())
            ));
        }

        let body: serde_json::Value = resp.json().await
            .map_err(|e| AsyncEmbedError::ProviderError(format!("engram /embed parse: {}", e)))?;

        let embedding: Vec<f32> = serde_json::from_value(body["embedding"].clone())
            .map_err(|e| AsyncEmbedError::ProviderError(format!("engram embedding field: {}", e)))?;

        if embedding.len() != self.dim {
            return Err(AsyncEmbedError::DimensionMismatch {
                expected: self.dim,
                got: embedding.len(),
            });
        }

        Ok(embedding)
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn name(&self) -> &str {
        "engram"
    }
}
