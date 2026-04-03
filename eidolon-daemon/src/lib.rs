pub mod absorber;
pub mod agents;
pub mod audit;
pub mod config;
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
    pub scrub_registry: Arc<Mutex<ScrubRegistry>>,
    pub rate_limiter: Option<Arc<rate_limit::RateLimiter>>,
    pub audit_log: Option<Arc<audit::AuditLog>>,
}

/// Get a 1024-dim embedding from Engram's /embed endpoint.
pub async fn embed_text(
    http: &reqwest::Client,
    engram_url: &str,
    engram_key: Option<&str>,
    text: &str,
) -> Option<Vec<f32>> {
    let url = format!("{}/embed", engram_url);
    let mut req = http
        .post(&url)
        .json(&serde_json::json!({"text": text}));
    if let Some(key) = engram_key {
        req = req.header("Authorization", format!("Bearer {}", key));
    }
    let resp = req.send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: serde_json::Value = resp.json().await.ok()?;
    serde_json::from_value(body["embedding"].clone()).ok()
}
