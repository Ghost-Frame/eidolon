use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;

mod absorber;
mod agents;
mod config;
mod prompt;
mod routes;
mod server;
mod session;

use config::Config;
use session::SessionManager;
use eidolon_lib::brain::Brain;

pub struct AppState {
    pub brain: Arc<Mutex<Brain>>,
    pub sessions: Arc<Mutex<SessionManager>>,
    pub config: Config,
    pub http_client: reqwest::Client,
}

#[tokio::main]
async fn main() {
    // Init tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("eidolon_daemon=info".parse().unwrap()),
        )
        .init();

    // Load config
    let config_path = std::env::args().nth(1);
    let config = match Config::load_or_default(config_path.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[eidolon-daemon] config error: {}", e);
            std::process::exit(1);
        }
    };

    tracing::info!("eidolon-daemon starting on {}:{}", config.server.host, config.server.port);
    tracing::info!("brain db: {}", config.brain.db_path);
    tracing::info!("engram url: {}", config.engram.url);

    // Init brain
    let mut brain = Brain::new();
    match brain.init(&config.brain.db_path, Some(&config.brain.data_dir)) {
        Ok(msg) => tracing::info!("brain: {}", msg),
        Err(e) => {
            tracing::warn!("brain init failed: {} -- continuing with empty brain", e);
        }
    }

    let state = Arc::new(AppState {
        brain: Arc::new(Mutex::new(brain)),
        sessions: Arc::new(Mutex::new(SessionManager::new())),
        http_client: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap(),
        config,
    });

    let bind_addr = format!("{}:{}", state.config.server.host, state.config.server.port);
    let router = server::build_router(Arc::clone(&state));

    let listener = match tokio::net::TcpListener::bind(&bind_addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[eidolon-daemon] failed to bind {}: {}", bind_addr, e);
            std::process::exit(1);
        }
    };

    tracing::info!("listening on {}", bind_addr);

    // Use into_make_service_with_connect_info so ConnectInfo<SocketAddr> is
    // available in middleware (needed for /gate/check localhost-only bypass).
    if let Err(e) = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    {
        eprintln!("[eidolon-daemon] server error: {}", e);
        std::process::exit(1);
    }
}
