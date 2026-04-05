use crate::app::App;
use crate::daemon::client::DaemonClient;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

/// Handle daemon-interactive slash commands. Returns true if the command was handled.
pub fn handle(
    app: &mut App,
    msg: &str,
    daemon_client: &Option<Arc<DaemonClient>>,
    system_tx: &mpsc::UnboundedSender<String>,
) -> bool {
    let parts: Vec<&str> = msg.split_whitespace().collect();
    let cmd = parts.first().copied().unwrap_or("");

    match cmd {
        "/daemon" => {
            if parts.get(1) == Some(&"reconnect") {
                if app.config.daemon.api_key.is_empty() {
                    app.add_system_message("No daemon.api_key configured. Add [daemon] section to config.toml.");
                    return true;
                }
                let url = app.config.daemon.url.clone();
                let key = app.config.daemon.api_key.clone();
                let (tx, rx) = oneshot::channel();
                app.daemon_reconnect_rx = Some(rx);
                app.add_system_message("Reconnecting to daemon...");
                tokio::spawn(async move {
                    let client = DaemonClient::new(&url, &key);
                    match client.health().await {
                        Ok(()) => { let _ = tx.send(Ok(client)); }
                        Err(e) => { let _ = tx.send(Err(e)); }
                    }
                });
            } else {
                let connected = daemon_client.is_some();
                app.add_system_message(&format!(
                    "Daemon: {}\nURL: {}\nUse /daemon reconnect to retry.",
                    if connected { "connected" } else { "disconnected" },
                    app.config.daemon.url
                ));
            }
            true
        }
        "/brain" => {
            if let Some(ref daemon) = daemon_client {
                let daemon = Arc::clone(daemon);
                let tx = system_tx.clone();
                tokio::spawn(async move {
                    match daemon.brain_stats().await {
                        Ok(stats) => {
                            let memories = stats["memory_count"].as_u64().unwrap_or(0);
                            let patterns = stats["pattern_count"].as_u64().unwrap_or(0);
                            let energy = stats["energy"].as_f64().unwrap_or(0.0);
                            let _ = tx.send(format!(
                                "Brain stats:\n  Memories: {}\n  Patterns: {}\n  Energy: {:.4}",
                                memories, patterns, energy
                            ));
                        }
                        Err(e) => { let _ = tx.send(format!("Brain stats failed: {}", e)); }
                    }
                });
            } else {
                app.add_system_message("Daemon not connected. Use /daemon reconnect.");
            }
            true
        }
        "/sessions" => {
            if let Some(ref daemon) = daemon_client {
                if parts.get(1) == Some(&"kill") {
                    if let Some(id) = parts.get(2) {
                        let daemon = Arc::clone(daemon);
                        let tx = system_tx.clone();
                        let session_id = id.to_string();
                        tokio::spawn(async move {
                            match daemon.kill_session(&session_id).await {
                                Ok(()) => { let _ = tx.send(format!("Session {} killed.", session_id)); }
                                Err(e) => { let _ = tx.send(format!("Kill failed: {}", e)); }
                            }
                        });
                    } else {
                        app.add_system_message("Usage: /sessions kill <session_id>");
                    }
                } else {
                    let daemon = Arc::clone(daemon);
                    let tx = system_tx.clone();
                    tokio::spawn(async move {
                        match daemon.list_sessions().await {
                            Ok(sessions) => {
                                let formatted = format_sessions(&sessions);
                                let _ = tx.send(formatted);
                            }
                            Err(e) => { let _ = tx.send(format!("List sessions failed: {}", e)); }
                        }
                    });
                }
            } else {
                app.add_system_message("Daemon not connected. Use /daemon reconnect.");
            }
            true
        }
        "/dream" => {
            if let Some(ref daemon) = daemon_client {
                let daemon = Arc::clone(daemon);
                let tx = system_tx.clone();
                tokio::spawn(async move {
                    match daemon.trigger_dream().await {
                        Ok(resp) => {
                            let consolidated = resp["consolidated"].as_u64().unwrap_or(0);
                            let pruned = resp["pruned"].as_u64().unwrap_or(0);
                            let _ = tx.send(format!(
                                "Dream cycle complete. Consolidated: {}, Pruned: {}",
                                consolidated, pruned
                            ));
                        }
                        Err(e) => { let _ = tx.send(format!("Dream cycle failed: {}", e)); }
                    }
                });
                app.add_system_message("Dream cycle triggered. Results incoming...");
            } else {
                app.add_system_message("Daemon not connected. Use /daemon reconnect.");
            }
            true
        }
        _ => false,
    }
}

fn format_sessions(sessions: &serde_json::Value) -> String {
    if let Some(arr) = sessions.as_array() {
        format_session_array(arr)
    } else if let Some(obj) = sessions.as_object() {
        if let Some(arr) = obj.get("sessions").and_then(|v| v.as_array()) {
            format_session_array(arr)
        } else {
            format!("Sessions: {}", sessions)
        }
    } else {
        format!("Sessions: {}", sessions)
    }
}

fn format_session_array(arr: &[serde_json::Value]) -> String {
    if arr.is_empty() {
        return "No active sessions.".to_string();
    }
    let mut lines = vec!["Sessions:".to_string()];
    for s in arr {
        let id = s["session_id"].as_str().unwrap_or("?");
        let status = s["status"].as_str().unwrap_or("?");
        let agent = s["agent"].as_str().unwrap_or("?");
        lines.push(format!("  {} [{}] ({})", id, status, agent));
    }
    lines.join("\n")
}
