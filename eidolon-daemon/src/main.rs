use std::sync::Arc;
use tokio::sync::Mutex;

use eidolon_daemon::*;
use eidolon_daemon::config::Config;
use eidolon_daemon::scrubbing::ScrubRegistry;
use eidolon_daemon::session::SessionManager;
use eidolon_lib::brain::Brain;
use eidolon_lib::growth;
use eidolon_lib::instincts;
use eidolon_daemon::config::EmbeddingConfig;
use eidolon_daemon::embedding::{self, AsyncEmbeddingProvider};

/// Build the embedding provider based on config.
fn build_embed_provider(
    config: &Config,
    http: &reqwest::Client,
) -> Arc<dyn AsyncEmbeddingProvider> {
    match &config.embedding {
        EmbeddingConfig::Engram { dim } => {
            tracing::info!("embedding provider: engram (dim={})", dim);
            Arc::new(embedding::engram::EngramProvider::new(
                http.clone(),
                config.engram.url.clone(),
                config.engram.api_key.clone(),
                *dim,
            ))
        }
        EmbeddingConfig::Openai { model, dim } => {
            let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_else(|_| {
                eprintln!("[eidolon-daemon] OPENAI_API_KEY required for openai embedding provider");
                std::process::exit(1);
            });
            tracing::info!("embedding provider: openai (model={}, dim={})",
                model.as_deref().unwrap_or("text-embedding-3-small"), dim);
            Arc::new(embedding::openai::OpenaiProvider::new(
                http.clone(),
                api_key,
                model.clone(),
                *dim,
            ))
        }
        EmbeddingConfig::Http { url, dim, auth_header } => {
            tracing::info!("embedding provider: http (url={}, dim={})", url, dim);
            Arc::new(embedding::http::HttpProvider::new(
                http.clone(),
                url.clone(),
                *dim,
                auth_header.clone(),
            ))
        }
    }
}

/// Enrich ghost instinct embeddings with real BGE-large vectors from Engram.
/// One-time migration: after first successful run, instincts.bin is version 2.
async fn enrich_instincts_if_needed(
    provider: &dyn AsyncEmbeddingProvider,
    config: &Config,
) {
    let instincts_path = format!("{}/instincts.bin", config.brain.data_dir);
    let mut corpus = match instincts::load_instincts(&instincts_path) {
        Some(c) => c,
        None => {
            tracing::debug!("no instincts.bin found at {} -- skipping enrichment", instincts_path);
            return;
        }
    };

    if corpus.version >= 2 {
        tracing::debug!("instincts already enriched (version {})", corpus.version);
        return;
    }

    tracing::info!(
        "enriching {} ghost instincts with real embeddings via {} provider",
        corpus.memories.len(),
        provider.name(),
    );

    let mut enriched = 0usize;
    let mut failed = 0usize;

    for memory in &mut corpus.memories {
        match provider.embed(&memory.content).await {
            Ok(embedding) => {
                memory.embedding = embedding;
                enriched += 1;
            }
            Err(_) => {
                failed += 1;
            }
        }
    }

    if enriched > 0 {
        corpus.version = 2;
        match instincts::save_instincts(&corpus, &instincts_path) {
            Ok(()) => {
                tracing::info!(
                    "instincts enriched: {} succeeded, {} failed (kept sine-wave fallback)",
                    enriched, failed
                );
            }
            Err(e) => {
                tracing::warn!("failed to save enriched instincts: {}", e);
            }
        }
    } else {
        tracing::warn!(
            "embedding provider unreachable -- all {} enrichment attempts failed, keeping sine-wave embeddings",
            failed
        );
    }
}

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
        eprintln!("[eidolon-daemon] no credd.agent_key and no EIDOLON_API_KEY - cannot start");
        std::process::exit(1);
    } else {
        tracing::warn!("no credd agent_key configured - using plaintext config (DEPRECATED)");
    }

    tracing::info!(
        "eidolon-daemon starting on {}:{}",
        config.server.host,
        config.server.port
    );
    tracing::info!("brain db: {}", config.brain.db_path);
    tracing::info!("engram url: {}", config.engram.url);
    tracing::info!("auth: {} API key(s) configured", config.auth.api_keys.len());

    // Build embedding provider
    let embed_provider = build_embed_provider(&config, &http_client);

    // Enrich ghost instinct embeddings with real vectors (one-time migration)
    enrich_instincts_if_needed(embed_provider.as_ref(), &config).await;

    // Init brain
    let mut brain = Brain::new();
    match brain.init(&config.brain.db_path, Some(&config.brain.data_dir)) {
        Ok(msg) => tracing::info!("brain: {}", msg),
        Err(e) => {
            tracing::warn!("brain init failed: {} - continuing with empty brain", e);
        }
    }

    // Init session manager with optional SQLite backing
    let session_db_path = config.sessions.db_path.clone();
    let session_manager = SessionManager::new(session_db_path.as_deref());

    // Init rate limiter (optional)
    let rate_limiter = config.rate_limit.as_ref().map(|rl| {
        tracing::info!(
            "rate limiting enabled: {} req/min + {} burst",
            rl.requests_per_minute,
            rl.burst
        );
        Arc::new(eidolon_daemon::rate_limit::RateLimiter::new(
            rl.requests_per_minute,
            rl.burst,
        ))
    });

    // Init audit log (optional)
    let audit_log = match config.audit.as_ref().and_then(|a| a.db_path.as_ref()) {
        Some(db_path) => match eidolon_daemon::audit::AuditLog::open(db_path) {
            Ok(log) => {
                tracing::info!("audit logging enabled: {}", db_path);
                Some(Arc::new(log))
            }
            Err(e) => {
                tracing::warn!("audit logging disabled (init failed): {}", e);
                None
            }
        },
        None => None,
    };

    let state = Arc::new(AppState {
        brain: Arc::new(Mutex::new(brain)),
        sessions: Arc::new(Mutex::new(session_manager)),
        http_client,
        config,
        embed_provider,
        scrub_registry: Arc::new(Mutex::new(ScrubRegistry::new())),
        rate_limiter,
        audit_log,
        pending_approvals: tokio::sync::Mutex::new(std::collections::HashMap::new()),
    });

    // Spawn dream cycle background task
    {
        let state_dream = Arc::clone(&state);
        let interval_secs = state.config.brain.dream_interval_secs;
        if interval_secs > 0 {
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
                interval.tick().await;
                loop {
                    interval.tick().await;
                    let (dream_result, pattern_count, edge_count) = {
                        let mut brain_guard = state_dream.brain.lock().await;
                        let result = brain_guard.run_dream_cycle();
                        tracing::info!(
                            "dream cycle: replayed={} merged={} pruned={} discovered={} decorrelated={} resolved={} ({}ms)",
                            result.replayed, result.merged, result.pruned_patterns,
                            result.discovered, result.decorrelated, result.resolved, result.cycle_time_ms
                        );

                        // Run evolution training step after each dream cycle
                        #[cfg(feature = "evolution")]
                        {
                            let gen = brain_guard.evolution_train();
                            tracing::info!("evolution: trained to generation {}", gen);
                        }

                        let stats = brain_guard.get_stats();
                        let pc = stats.total_patterns;
                        let ec = stats.total_edges;
                        (result, pc, ec)
                    };

                    // Growth reflection after dream cycle (probabilistic)
                    if growth::should_reflect(&state_dream.config.growth) {
                        let dr = &dream_result;
                        let context = growth::build_dream_context(
                            dr.replayed, dr.merged, dr.pruned_patterns, dr.pruned_edges,
                            dr.discovered, dr.decorrelated, dr.resolved, dr.cycle_time_ms,
                            pattern_count, edge_count,
                        );

                        match growth::reflect(
                            &state_dream.http_client,
                            &state_dream.config.growth,
                            "eidolon",
                            &context,
                            None,
                            None,
                        ).await {
                            Ok(Some(obs)) => {
                                tracing::info!(observation = %obs, "growth: dream reflection recorded");
                                let activity = serde_json::json!({
                                    "agent": "eidolon-daemon",
                                    "action": "growth.observed",
                                    "summary": obs,
                                });
                                let _ = state_dream.http_client
                                    .post(format!("http://127.0.0.1:{}/activity", state_dream.config.server.port))
                                    .bearer_auth(state_dream.config.auth.api_keys.first().map(|k| k.key.as_str()).unwrap_or(""))
                                    .json(&activity)
                                    .send().await;
                            }
                            Ok(None) => {}
                            Err(e) => tracing::warn!(error = %e, "growth: dream reflection failed"),
                        }
                    }

                    // Evict terminal sessions older than 1 hour
                    {
                        let mut sessions = state_dream.sessions.lock().await;
                        sessions.evict_completed(std::time::Duration::from_secs(3600));
                    }
                }
            });
        }
    }

    // Spawn rate limiter cleanup task (prune expired entries every 5 minutes)
    if let Some(ref limiter) = state.rate_limiter {
        let limiter = Arc::clone(limiter);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
            interval.tick().await;
            loop {
                interval.tick().await;
                limiter.prune_expired();
            }
        });
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
