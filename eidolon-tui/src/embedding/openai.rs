use super::{AsyncEmbedError, AsyncEmbeddingProvider};

/// OpenAI embeddings API provider.
pub struct OpenaiProvider {
    http: reqwest::Client,
    api_key: String,
    model: String,
    dim: usize,
}

impl OpenaiProvider {
    pub fn new(http: reqwest::Client, api_key: String, model: Option<String>, dim: usize) -> Self {
        OpenaiProvider {
            http,
            api_key,
            model: model.unwrap_or_else(|| "text-embedding-3-small".to_string()),
            dim,
        }
    }
}

#[async_trait::async_trait]
impl AsyncEmbeddingProvider for OpenaiProvider {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, AsyncEmbedError> {
        let url = "https://api.openai.com/v1/embeddings";
        let resp = self.http.post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&serde_json::json!({
                "input": text,
                "model": self.model,
            }))
            .send()
            .await
            .map_err(|e| AsyncEmbedError::NetworkError(format!("openai: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AsyncEmbedError::ProviderError(
                format!("openai returned {}: {}", status, body)
            ));
        }

        let body: serde_json::Value = resp.json().await
            .map_err(|e| AsyncEmbedError::ProviderError(format!("openai parse: {}", e)))?;

        let embedding: Vec<f32> = body["data"][0]["embedding"]
            .as_array()
            .ok_or_else(|| AsyncEmbedError::ProviderError("missing embedding array".to_string()))?
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();

        if embedding.len() != self.dim {
            return Err(AsyncEmbedError::DimensionMismatch {
                expected: self.dim,
                got: embedding.len(),
            });
        }

        Ok(embedding)
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, AsyncEmbedError> {
        let url = "https://api.openai.com/v1/embeddings";
        let resp = self.http.post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&serde_json::json!({
                "input": texts,
                "model": self.model,
            }))
            .send()
            .await
            .map_err(|e| AsyncEmbedError::NetworkError(format!("openai batch: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AsyncEmbedError::ProviderError(
                format!("openai batch returned {}: {}", status, body)
            ));
        }

        let body: serde_json::Value = resp.json().await
            .map_err(|e| AsyncEmbedError::ProviderError(format!("openai batch parse: {}", e)))?;

        let data = body["data"].as_array()
            .ok_or_else(|| AsyncEmbedError::ProviderError("missing data array".to_string()))?;

        let mut results = Vec::with_capacity(data.len());
        for item in data {
            let embedding: Vec<f32> = item["embedding"]
                .as_array()
                .ok_or_else(|| AsyncEmbedError::ProviderError("missing embedding".to_string()))?
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect();

            if embedding.len() != self.dim {
                return Err(AsyncEmbedError::DimensionMismatch {
                    expected: self.dim,
                    got: embedding.len(),
                });
            }
            results.push(embedding);
        }

        Ok(results)
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn name(&self) -> &str {
        "openai"
    }
}
