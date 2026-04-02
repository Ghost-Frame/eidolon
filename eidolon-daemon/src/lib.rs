pub mod absorber;
pub mod agents;
pub mod config;
pub mod prompt;
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

pub struct AppState {
    pub brain: Arc<Mutex<Brain>>,
    pub sessions: Arc<Mutex<SessionManager>>,
    pub config: Config,
    pub http_client: reqwest::Client,
    pub scrub_registry: Arc<Mutex<ScrubRegistry>>,
}
