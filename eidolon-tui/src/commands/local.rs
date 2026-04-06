use crate::app::{App, ClaudeSessionState};
use crate::config::resolve_model_alias;

/// Handle local (non-daemon) slash commands. Returns true if handled.
pub fn handle(app: &mut App, msg: &str) -> bool {
    let parts: Vec<&str> = msg.split_whitespace().collect();
    let cmd = parts.first().copied().unwrap_or("");

    match cmd {
        "/theme" => {
            if let Some(name) = parts.get(1) {
                if crate::tui::theme::Theme::by_name(name).is_some() {
                    app.cycle_theme();
                    app.add_system_message("Theme switched. Looking good.");
                } else {
                    app.add_system_message(&format!("No theme called '{}'. Nice try.", name));
                }
            } else {
                app.add_system_message(&format!("Current theme: {}", app.theme.name));
            }
            true
        }
        "/model" | "/models" => {
            if parts.len() == 1 {
                let c = &app.config.agents.claude;
                let x = &app.config.agents.codex;
                app.add_system_message(&format!(
                    "Claude models:\n  light  -> {}\n  medium -> {}\n  heavy  -> {}\n\nCodex models:\n  light  -> {}\n  medium -> {}\n  heavy  -> {}\n\nSet: /model <claude|codex> <light|medium|heavy> <model>\nAliases: haiku, sonnet, opus, 5.4, 5.4-mini, 5.3-codex, 5.2-codex, 5.2, 5.1-max, 5.1-mini",
                    c.model_light, c.model_medium, c.model_heavy,
                    x.model_light, x.model_medium, x.model_heavy
                ));
            } else if parts.len() >= 4 {
                let agent = parts[1];
                let tier = parts[2];
                let model = resolve_model_alias(parts[3]);
                let entry = match agent {
                    "claude" => Some(&mut app.config.agents.claude),
                    "codex" => Some(&mut app.config.agents.codex),
                    _ => None,
                };
                if let Some(entry) = entry {
                    match tier {
                        "light" => {
                            entry.model_light = model.clone();
                            app.add_system_message(&format!("{} light -> {}", agent, model));
                        }
                        "medium" => {
                            entry.model_medium = model.clone();
                            app.add_system_message(&format!("{} medium -> {}", agent, model));
                        }
                        "heavy" => {
                            entry.model_heavy = model.clone();
                            app.add_system_message(&format!("{} heavy -> {}", agent, model));
                        }
                        _ => {
                            app.add_system_message("Tiers: light, medium, heavy");
                        }
                    }
                } else {
                    app.add_system_message("Agents: claude, codex");
                }
            } else {
                app.add_system_message("Usage: /model <claude|codex> <light|medium|heavy> <model>");
            }
            true
        }
        "/status" => {
            let sidecar = format!("{:?}", app.sidecar_status);
            let c = &app.config.agents.claude;
            let daemon_status = format!("Daemon: {} ({})",
                if app.config.daemon.api_key.is_empty() { "not configured" } else { "configured" },
                app.config.daemon.url
            );
            let claude_state = match &app.claude_session.state {
                ClaudeSessionState::Idle => "idle".to_string(),
                ClaudeSessionState::Starting => "starting".to_string(),
                ClaudeSessionState::Active { session_id, .. } => format!("active ({})", session_id),
                ClaudeSessionState::Completed { exit_code, .. } => format!("completed (exit {})", exit_code),
                ClaudeSessionState::Failed { error } => format!("failed: {}", error),
            };
            app.add_system_message(&format!(
                "LLM: {}\nEngram: {}\n{}\nModels: light={}, medium={}, heavy={}\nClaude panel: {}",
                sidecar, app.config.engram.url, daemon_status,
                c.model_light, c.model_medium, c.model_heavy,
                claude_state
            ));
            true
        }
        "/help" => {
            app.add_system_message(
                "Commands:\n  /model             - show/set model tiers\n  /status            - system status\n  /theme <name>      - switch theme\n  /daemon            - daemon connection status\n  /daemon reconnect  - reconnect to daemon\n  /brain             - brain stats from daemon\n  /sessions          - list daemon sessions\n  /sessions kill <id> - kill a session\n  /dream             - trigger dream cycle\n  /claude            - show Claude session state\n  /claude kill       - kill active Claude session\n  /claude clear      - clear Claude panel\n  /clear             - clear chat (or Ctrl+L)\n  /quit              - exit"
            );
            true
        }
        "/clear" => {
            app.clear_messages();
            app.add_system_message("Screen cleared. I'm still here though. Obviously.");
            true
        }
        "/claude" => {
            let subcommand = parts.get(1).copied().unwrap_or("");
            match subcommand {
                "clear" => {
                    app.claude_session.clear();
                    app.add_system_message("Claude panel cleared.");
                }
                "" => {
                    let state_str = match &app.claude_session.state {
                        ClaudeSessionState::Idle => "idle".to_string(),
                        ClaudeSessionState::Starting => "starting...".to_string(),
                        ClaudeSessionState::Active { session_id, .. } => {
                            format!("active (session: {})", session_id)
                        }
                        ClaudeSessionState::Completed { session_id, exit_code } => {
                            format!("completed (session: {}, exit: {})", session_id, exit_code)
                        }
                        ClaudeSessionState::Failed { error } => {
                            format!("failed: {}", error)
                        }
                    };
                    let model = if app.claude_session.model.is_empty() {
                        "none".to_string()
                    } else {
                        app.claude_session.model.clone()
                    };
                    let msgs = app.claude_session.messages.len();
                    app.add_system_message(&format!(
                        "Claude session: {}\nModel: {}\nMessages: {}",
                        state_str, model, msgs
                    ));
                }
                _ => {
                    app.add_system_message("Usage: /claude, /claude clear, /claude kill");
                }
            }
            true
        }
        "/quit" | "/exit" => {
            app.should_quit = true;
            true
        }
        _ => {
            app.add_system_message(&format!("Unknown command: {}. Try /help", cmd));
            true
        }
    }
}
