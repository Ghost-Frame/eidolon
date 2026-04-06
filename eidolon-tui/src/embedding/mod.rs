pub mod engram;
pub mod http;
pub mod openai;

use std::fmt;

/// Error type for async embedding operations.
#[derive(Debug)]
pub enum AsyncEmbedError {
    DimensionMismatch { expected: usize, got: usize },
    ProviderError(String),
    NetworkError(String),
}

impl fmt::Display for AsyncEmbedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AsyncEmbedError::DimensionMismatch { expected, got } =>
                write!(f, "dimension mismatch: expected {}, got {}", expected, got),
            AsyncEmbedError::ProviderError(msg) => write!(f, "provider error: {}", msg),
            AsyncEmbedError::NetworkError(msg) => write!(f, "network error: {}", msg),
        }
    }
}

impl std::error::Error for AsyncEmbedError {}

/// Async embedding provider trait -- mirrors eidolon-daemon's trait.
/// HTTP-based providers (Engram, OpenAI, generic HTTP) implement this.
#[async_trait::async_trait]
pub trait AsyncEmbeddingProvider: Send + Sync {
    /// Embed a single text string.
    async fn embed(&self, text: &str) -> Result<Vec<f32>, AsyncEmbedError>;

    /// Embed a batch of texts. Default calls embed() sequentially.
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, AsyncEmbedError> {
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.embed(text).await?);
        }
        Ok(results)
    }

    /// The dimensionality of embeddings produced by this provider.
    fn dim(&self) -> usize;

    /// Human-readable name for this provider.
    fn name(&self) -> &str;
}
