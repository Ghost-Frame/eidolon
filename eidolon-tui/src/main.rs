use eidolon_tui::config::Config;
use eidolon_tui::event_loop;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Config::load().unwrap_or_default();

    // Bootstrap API keys from credd if configured
    if let Err(e) = config.bootstrap_from_credd().await {
        eprintln!("[eidolon-tui] credd bootstrap: {} (using config fallback)", e);
    }

    #[cfg(unix)]
    Config::check_file_permissions();

    event_loop::run(config).await
}
