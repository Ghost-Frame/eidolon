use std::sync::Arc;
use tokio::sync::Mutex;

use eidolon_daemon::*;
use eidolon_daemon::config::Config;
use eidolon_daemon::scrubbing::ScrubRegistry;
use eidolon_daemon::session::SessionManager;
use eidolon_lib::brain::Brain;

#[tokio::main]
async fn main() {
    // Init tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("eidolon_daemon=info".parse().unwrap()),
        )
        .init();

    // Load config (phase 1: sync, reads TOML)
    let config_path = std::env::args().nth(1);
    let mut config = match Config::load_or_default(config_path.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[eidolon-daemon] config error: {}", e);
            std::process::exit(1);
        }
    };

    // Phase 2: async bootstrap secrets from credd
    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap();

    if config.credd.agent_key.is_some() {
        match config.bootstrap_from_credd(&http_client).await {
            Ok(()) => tracing::info!("secrets bootstrapped from credd"),
            Err(e) => {
                if config.api_key.is_empty() || config.engram.api_key.is_none() {
                    eprintln!(
                        "[eidolon-daemon] credd bootstrap failed and no fallback keys: {}",
                        e
                    );
                    std::process::exit(1);
                }
                tracing::warn!("credd bootstrap failed (using config fallbacks): {}", e);
            }
        }
    } else if config.api_key.is_empty() {
        eprintln!("[eidolon-daemon] no credd.agent_key and no EIDOLON_API_KEY -- cannot start");
        std::process::exit(1);
    } else {
        tracing::warn!("no credd agent_key configured -- using plaintext config (DEPRECATED)");
    }

    tracing::info!(
        "eidolon-daemon starting on {}:{}",
        config.server.host,
        config.server.port
    );
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
        http_client,
        config,
        scrub_registry: Arc::new(Mutex::new(ScrubRegistry::new())),
    });

    // Spawn dream cycle background task
    {
        let brain = Arc::clone(&state.brain);
        let interval_secs = state.config.brain.dream_interval_secs;
        if interval_secs > 0 {
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
                interval.tick().await; // Skip first immediate tick
                loop {
                    interval.tick().await;
                    let mut brain_guard = brain.lock().await;
                    let result = brain_guard.run_dream_cycle();
                    tracing::info!(
                        "dream cycle: replayed={} merged={} pruned={} discovered={} resolved={} ({}ms)",
                        result.replayed, result.merged, result.pruned_patterns,
                        result.discovered, result.resolved, result.cycle_time_ms
                    );
                }
            });
        }
    }

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

    if let Err(e) = axum::serve(listener, router.into_make_service_with_connect_info::<std::net::SocketAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await
    {
        eprintln!("[eidolon-daemon] server error: {}", e);
        std::process::exit(1);
    }

    tracing::info!("eidolon-daemon shutting down gracefully");
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => { tracing::info!("received Ctrl+C"); },
        _ = terminate => { tracing::info!("received SIGTERM"); },
    }
}
