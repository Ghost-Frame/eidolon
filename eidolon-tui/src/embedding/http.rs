use super::{AsyncEmbedError, AsyncEmbeddingProvider};

/// Generic HTTP embedding provider. Sends POST requests with {"text": ...}
/// and expects {"embedding": [...]} in the response.
pub struct HttpProvider {
    http: reqwest::Client,
    url: String,
    dim: usize,
    auth_header: Option<String>,
}

impl HttpProvider {
    pub fn new(http: reqwest::Client, url: String, dim: usize, auth_header: Option<String>) -> Self {
        HttpProvider { http, url, dim, auth_header }
    }
}

#[async_trait::async_trait]
impl AsyncEmbeddingProvider for HttpProvider {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, AsyncEmbedError> {
        let mut req = self.http.post(&self.url)
            .json(&serde_json::json!({"text": text}));

        if let Some(ref auth) = self.auth_header {
            req = req.header("Authorization", auth.as_str());
        }

        let resp = req.send().await
            .map_err(|e| AsyncEmbedError::NetworkError(format!("http embed: {}", e)))?;

        if !resp.status().is_success() {
            return Err(AsyncEmbedError::ProviderError(
                format!("http embed returned {}", resp.status())
            ));
        }

        let body: serde_json::Value = resp.json().await
            .map_err(|e| AsyncEmbedError::ProviderError(format!("http embed parse: {}", e)))?;

        let embedding: Vec<f32> = serde_json::from_value(body["embedding"].clone())
            .map_err(|e| AsyncEmbedError::ProviderError(format!("embedding field: {}", e)))?;

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
        "http"
    }
}
