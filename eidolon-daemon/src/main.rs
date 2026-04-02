use std::sync::Arc;
use tokio::sync::Mutex;

use eidolon_daemon::*;
use eidolon_daemon::config::Config;
use eidolon_daemon::scrubbing::ScrubRegistry;
use eidolon_daemon::session::SessionManager;
use eidolon_lib::brain::Brain;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("eidolon_daemon=info".parse().unwrap()),
        )
        .init();

    let config_path = std::env::args().nth(1);
    let mut config = match Config::load_or_default(config_path.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[eidolon-daemon] config error: {}", e);
            std::process::exit(1);
        }
    };

    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap();

    if config.credd.agent_key.is_some() {
        match config.bootstrap_from_credd(&http_client).await {
            Ok(()) => tracing::info!("secrets bootstrapped from credd"),
            Err(e) => {
                if !config.auth.has_keys() || config.engram.api_key.is_none() {
                    eprintln!(
                        "[eidolon-daemon] credd bootstrap failed and no fallback keys: {}",
                        e
                    );
                    std::process::exit(1);
                }
                tracing::warn!("credd bootstrap failed (using config fallbacks): {}", e);
            }
        }
    } else if !config.auth.has_keys() {
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
    tracing::info!("auth: {} API key(s) configured", config.auth.api_keys.len());

    // Init brain
    let mut brain = Brain::new();
    match brain.init(&config.brain.db_path, Some(&config.brain.data_dir)) {
        Ok(msg) => tracing::info!("brain: {}", msg),
        Err(e) => {
            tracing::warn!("brain init failed: {} -- continuing with empty brain", e);
        }
    }

    // Init session manager with optional SQLite backing
    let session_db_path = config.sessions.db_path.clone();
    let session_manager = SessionManager::new(session_db_path.as_deref());

    let state = Arc::new(AppState {
        brain: Arc::new(Mutex::new(brain)),
        sessions: Arc::new(Mutex::new(session_manager)),
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
                interval.tick().await;
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

    if let Some(ref tls_config) = state.config.tls {
        tracing::info!("TLS enabled: cert={} key={}", tls_config.cert_path, tls_config.key_path);

        let tls_acceptor = match build_tls_acceptor(&tls_config.cert_path, &tls_config.key_path) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("[eidolon-daemon] TLS setup failed: {}", e);
                std::process::exit(1);
            }
        };

        tracing::info!("listening on {} (TLS)", bind_addr);
        serve_tls(listener, tls_acceptor, router).await;
    } else {
        tracing::info!("listening on {} (plain HTTP)", bind_addr);

        if let Err(e) = axum::serve(listener, router.into_make_service_with_connect_info::<std::net::SocketAddr>())
            .with_graceful_shutdown(shutdown_signal())
            .await
        {
            eprintln!("[eidolon-daemon] server error: {}", e);
            std::process::exit(1);
        }
    }

    tracing::info!("eidolon-daemon shutting down gracefully");
}

fn build_tls_acceptor(cert_path: &str, key_path: &str) -> Result<tokio_rustls::TlsAcceptor, String> {
    use rustls::ServerConfig;
    use std::io::BufReader;

    let cert_file = std::fs::File::open(cert_path)
        .map_err(|e| format!("failed to open cert {}: {}", cert_path, e))?;
    let key_file = std::fs::File::open(key_path)
        .map_err(|e| format!("failed to open key {}: {}", key_path, e))?;

    let certs: Vec<rustls::pki_types::CertificateDer<'static>> =
        rustls_pemfile::certs(&mut BufReader::new(cert_file))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("failed to parse certs: {}", e))?;

    if certs.is_empty() {
        return Err("no certificates found in cert file".to_string());
    }

    let key = rustls_pemfile::private_key(&mut BufReader::new(key_file))
        .map_err(|e| format!("failed to parse private key: {}", e))?
        .ok_or_else(|| "no private key found in key file".to_string())?;

    let server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| format!("TLS config error: {}", e))?;

    Ok(tokio_rustls::TlsAcceptor::from(Arc::new(server_config)))
}

async fn serve_tls(
    listener: tokio::net::TcpListener,
    tls_acceptor: tokio_rustls::TlsAcceptor,
    router: axum::Router,
) {
    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((tcp_stream, remote_addr)) => {
                        let acceptor = tls_acceptor.clone();
                        let app = router.clone();

                        tokio::spawn(async move {
                            let tls_stream = match acceptor.accept(tcp_stream).await {
                                Ok(s) => s,
                                Err(e) => {
                                    tracing::debug!("TLS handshake failed from {}: {}", remote_addr, e);
                                    return;
                                }
                            };

                            // Inject ConnectInfo into the request extensions manually
                            let svc = app.into_service();
                            let io = hyper_util::rt::TokioIo::new(tls_stream);
                            let hyper_svc = hyper::service::service_fn(move |mut req: hyper::Request<hyper::body::Incoming>| {
                                // Insert ConnectInfo so routes can extract remote addr
                                req.extensions_mut().insert(axum::extract::ConnectInfo(remote_addr));
                                let mut svc = svc.clone();
                                async move {
                                    use tower::Service;
                                    svc.call(req).await
                                }
                            });

                            if let Err(e) = hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new())
                                .serve_connection(io, hyper_svc)
                                .await
                            {
                                tracing::debug!("TLS connection error from {}: {}", remote_addr, e);
                            }
                        });
                    }
                    Err(e) => {
                        tracing::warn!("accept error: {}", e);
                    }
                }
            }
            _ = shutdown_signal() => {
                tracing::info!("shutting down TLS server");
                break;
            }
        }
    }
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
