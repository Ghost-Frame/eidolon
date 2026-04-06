pub mod absorber;
pub mod agents;
pub mod audit;
pub mod config;
pub mod embedding;
pub mod prompt;
pub mod rate_limit;
pub mod routes;
pub mod scrubbing;
pub mod secrets;
pub mod server;
pub mod session;

use std::sync::Arc;
use tokio::sync::Mutex;

use config::Config;
use scrubbing::ScrubRegistry;
use session::SessionManager;
use eidolon_lib::brain::Brain;

/// User identity extracted from auth middleware.
/// Injected into request extensions after API key validation.
#[derive(Debug, Clone)]
pub struct UserIdentity(pub String);

pub struct AppState {
    pub brain: Arc<Mutex<Brain>>,
    pub sessions: Arc<Mutex<SessionManager>>,
    pub config: Config,
    pub http_client: reqwest::Client,
    pub embed_provider: Arc<dyn embedding::AsyncEmbeddingProvider>,
    pub scrub_registry: Arc<Mutex<ScrubRegistry>>,
    pub rate_limiter: Option<Arc<rate_limit::RateLimiter>>,
    pub audit_log: Option<Arc<audit::AuditLog>>,
}

impl AppState {
    /// Embed text using the configured provider. Returns None on failure.
    pub async fn embed_text(&self, text: &str) -> Option<Vec<f32>> {
        match self.embed_provider.embed(text).await {
            Ok(v) => Some(v),
            Err(e) => {
                tracing::warn!("embed_text failed: {}", e);
                None
            }
        }
    }

    /// Check if the configured embedding provider is Engram.
    pub fn engram_enabled(&self) -> bool {
        self.embed_provider.name() == "engram"
    }
}
