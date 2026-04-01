use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

mod app;

use app::{App, AppMode};
use eidolon_tui::agents::orchestrator::{AgentOrchestrator, AgentType};
use eidolon_tui::config::Config;
use eidolon_tui::conversation::personality::gojo_system_prompt;
use eidolon_tui::conversation::router::RoutingDecision;
use eidolon_tui::llm::client::LlmClient;
use eidolon_tui::llm::sidecar::{LlamaSidecar, SidecarStatus};
use reqwest;
use eidolon_tui::syntheos::engram::EngramClient;
use eidolon_tui::tui::terminal;
use eidolon_tui::tui::widgets::{
    chat_area::ChatArea,
    input_bar::InputBar,
    status_bar::StatusBar,
    thinking::ThinkingSpinner,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load().unwrap_or_default();

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = terminal::restore();
        default_hook(info);
    }));

    let mut tui = terminal::init()?;
    let system_prompt = gojo_system_prompt();
    let mut app = App::new(config.clone(), system_prompt.clone());

    let llm_base_url = app.config.llm.base_url.clone()
        .unwrap_or_else(|| format!("http://localhost:{}", app.config.llm.port));

    let mut llm_client = Arc::new(LlmClient::new(&llm_base_url));

    let engram_client = Arc::new(EngramClient::new(
        &app.config.engram.url,
        &app.config.engram.api_key,
    ));

    let mut orchestrator = AgentOrchestrator::new(app.config.agents.clone());

    // Spawn llama-server sidecar if needed -- result piped through channel
    let mut sidecar_result_rx: Option<oneshot::Receiver<(SidecarStatus, u16)>> = None;
    if app.config.llm.base_url.is_none() && !app.config.llm.model_path.is_empty() {
        let server_path = app.config.llm.server_path.clone();
        let model_path = app.config.llm.model_path.clone();
        let port = app.config.llm.port;
        let ctx = app.config.llm.context_length;
        let ngl = app.config.llm.gpu_layers;
        let (tx, rx) = oneshot::channel();
        sidecar_result_rx = Some(rx);
        tokio::spawn(async move {
            let mut sidecar = LlamaSidecar::new(&server_path, &model_path, port, ctx, ngl);
            let result = if sidecar.start().await.is_ok() {
                SidecarStatus::Ready
            } else {
                sidecar.status().clone()
            };
            let actual_port = sidecar.port();
            let _ = tx.send((result, actual_port));
            // Keep sidecar alive (Drop kills the process)
            loop { tokio::time::sleep(Duration::from_secs(3600)).await; }
        });
    } else if app.config.llm.base_url.is_some() {
        app.sidecar_status = SidecarStatus::Ready;
    }

    // Health check runs in background -- we poll the result each frame
    let mut health_check_rx: Option<oneshot::Receiver<bool>> = None;
    let mut last_health_check = std::time::Instant::now();

    loop {
        if app.should_quit {
            break;
        }

        // --- POLL SIDECAR STARTUP RESULT ---
        if let Some(rx) = sidecar_result_rx.as_mut() {
            match rx.try_recv() {
                Ok((status, actual_port)) => {
                    if let SidecarStatus::Error(ref e) = status {
                        app.add_gojo_message(&format!("LLM failed to start: {}", e));
                    }
                    // Recreate LlmClient if sidecar bound a different port
                    if actual_port != app.config.llm.port && status == SidecarStatus::Ready {
                        let new_url = format!("http://localhost:{}", actual_port);
                        llm_client = Arc::new(LlmClient::new(&new_url));
                        app.config.llm.port = actual_port;
                    }
                    app.sidecar_status = status;
                    sidecar_result_rx = None;
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
                Err(oneshot::error::TryRecvError::Closed) => {
                    app.sidecar_status = SidecarStatus::Error("Sidecar task crashed".to_string());
                    sidecar_result_rx = None;
                }
            }
        }

        // --- POLL LLM HEALTH (background task, non-blocking) ---
        if app.sidecar_status != SidecarStatus::Ready && !matches!(app.sidecar_status, SidecarStatus::Error(_)) {
            // Collect result from in-flight check
            if let Some(rx) = health_check_rx.as_mut() {
                match rx.try_recv() {
                    Ok(true) => {
                        app.sidecar_status = SidecarStatus::Ready;
                        health_check_rx = None;
                    }
                    Ok(false) => {
                        health_check_rx = None;
                    }
                    Err(oneshot::error::TryRecvError::Empty) => {}
                    Err(oneshot::error::TryRecvError::Closed) => {
                        health_check_rx = None;
                    }
                }
            }

            // Kick off a new check every 2s if none in flight
            if health_check_rx.is_none() {
                let now = std::time::Instant::now();
                if now.duration_since(last_health_check) >= Duration::from_secs(2) {
                    last_health_check = now;
                    let port = app.config.llm.port;
                    let (tx, rx) = oneshot::channel();
                    health_check_rx = Some(rx);
                    tokio::spawn(async move {
                        let url = format!("http://localhost:{}/health", port);
                        let client = reqwest::Client::new();
                        let healthy = tokio::time::timeout(
                            Duration::from_millis(1500),
                            client.get(&url).send(),
                        )
                        .await
                        .ok()
                        .and_then(|r| r.ok())
                        .map(|r| r.status().is_success())
                        .unwrap_or(false);
                        let _ = tx.send(healthy);
                    });
                }
            }
        }

        // --- RENDER ---
        tui.draw(|frame| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Min(5),
                    Constraint::Length(3),
                ])
                .split(frame.area());

            let llm_status = app.llm_status();
            let status = StatusBar::new(
                app.theme,
                &app.animation,
                &llm_status,
                app.context_tokens,
                app.config.llm.context_length,
                0,
            );
            frame.render_widget(status, chunks[0]);

            let mut display_messages = app.chat_messages.clone();
            if (app.mode == AppMode::Generating || app.mode == AppMode::Routing)
                && !app.pending_response.is_empty()
            {
                display_messages.push(eidolon_tui::tui::widgets::chat_area::ChatMessage {
                    sender: "Gojo".to_string(),
                    content: app.pending_response.clone(),
                    is_user: false,
                });
            }
            let chat = ChatArea::new(app.theme, &display_messages, app.scroll_offset);
            frame.render_widget(chat, chunks[1]);

            if app.mode == AppMode::Generating || app.mode == AppMode::Routing {
                let spinner = ThinkingSpinner::new(app.theme, &app.animation);
                let spinner_area = ratatui::layout::Rect {
                    x: chunks[1].x + 2,
                    y: chunks[1].bottom().saturating_sub(1),
                    width: chunks[1].width.saturating_sub(4),
                    height: 1,
                };
                frame.render_widget(spinner, spinner_area);
            }

            let input = InputBar::new(app.theme, &app.input, app.cursor_pos);
            frame.render_widget(input, chunks[2]);
        })?;

        // --- POLL ROUTING RESULT ---
        if let Some(rx) = app.routing_rx.as_mut() {
            match rx.try_recv() {
                Ok(result) => {
                    app.routing_rx = None;
                    match result {
                        Ok(decision) => {
                            let user_msg = app.pending_user_message.clone();
                            match decision.intent {
                                eidolon_tui::conversation::router::Intent::Casual => {
                                    // Stream already running -- just record the decision
                                    app.pending_decision = Some(decision);
                                }
                                eidolon_tui::conversation::router::Intent::Memory => {
                                    // Abort the optimistic casual stream
                                    if let Some(abort) = app.stream_abort.take() {
                                        abort.abort();
                                    }
                                    app.pending_response.clear();
                                    app.pending_decision = Some(decision.clone());
                                    let client = llm_client.clone();
                                    let engram = engram_client.clone();
                                    let sys = system_prompt.clone();
                                    let history = build_history(&app);
                                    let model = app.config.llm.model_name.clone();
                                    let temp = app.config.llm.temperature_casual;
                                    let (tx, rx) = mpsc::unbounded_channel();
                                    app.start_streaming(rx);

                                    tokio::spawn(async move {
                                        // Search Engram for relevant context
                                        let context = match engram.search(&user_msg, 5).await {
                                            Ok(results) if !results.is_empty() => {
                                                format!("\n\n[Memory context]\n{}", results.join("\n"))
                                            }
                                            _ => String::new(),
                                        };

                                        // Inject context into user message
                                        let augmented = if context.is_empty() {
                                            user_msg.clone()
                                        } else {
                                            format!("{}{}", user_msg, context)
                                        };

                                        let mut msgs: Vec<(&str, String)> = vec![("system", sys)];
                                        for (role, content) in &history {
                                            msgs.push((role.as_str(), content.clone()));
                                        }
                                        // Replace last user message with augmented version
                                        if let Some(last) = msgs.last_mut() {
                                            if last.0 == "user" {
                                                last.1 = augmented;
                                            }
                                        }

                                        let msg_refs: Vec<(&str, &str)> = msgs.iter()
                                            .map(|(r, c)| (*r, c.as_str()))
                                            .collect();
                                        let request = LlmClient::build_request_with_model(&model, &msg_refs, temp, None);
                                        let _ = client.stream_complete(&request, tx).await;
                                    });
                                }
                                eidolon_tui::conversation::router::Intent::Action => {
                                    // Abort the optimistic casual stream
                                    if let Some(abort) = app.stream_abort.take() {
                                        abort.abort();
                                    }
                                    app.pending_response.clear();
                                    app.token_rx = None;
                                    // Store decision, show confirmation prompt
                                    let agent = decision.agent_needed.clone()
                                        .unwrap_or_else(|| "claude".to_string());
                                    let model = decision.select_model(&app.config.agents);
                                    let complexity = format!("{:?}", decision.complexity).to_lowercase();
                                    let reasoning = decision.reasoning.clone();
                                    app.pending_decision = Some(decision);
                                    app.mode = AppMode::AwaitingConfirmation;
                                    app.add_gojo_message(&format!(
                                        "I'd use {} for this ({} complexity -> {}). {}\n\nSay yes to proceed, or tell me what to do differently.",
                                        agent, complexity, model, reasoning
                                    ));
                                }
                            }
                        }
                        Err(e) => {
                            // Routing failed -- use keyword fallback to detect action intent
                            let fallback = RoutingDecision::keyword_fallback(&app.pending_user_message);
                            match fallback.intent {
                                eidolon_tui::conversation::router::Intent::Action => {
                                    // Abort optimistic casual stream, show agent confirmation
                                    if let Some(abort) = app.stream_abort.take() {
                                        abort.abort();
                                    }
                                    app.pending_response.clear();
                                    app.token_rx = None;
                                    let agent = fallback.agent_needed.clone()
                                        .unwrap_or_else(|| "claude".to_string());
                                    let model = fallback.select_model(&app.config.agents);
                                    app.pending_decision = Some(fallback);
                                    app.mode = AppMode::AwaitingConfirmation;
                                    app.add_gojo_message(&format!(
                                        "Router hiccup ({}), but this looks like an action. Use {} (-> {})?\n\nSay yes to proceed.",
                                        e, agent, model
                                    ));
                                }
                                _ => {
                                    // Casual stream already running, let it finish
                                    app.mode = AppMode::Generating;
                                }
                            }
                        }
                    }
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
                Err(oneshot::error::TryRecvError::Closed) => {
                    app.routing_rx = None;
                    app.mode = AppMode::Normal;
                }
            }
        }

        // --- DRAIN STREAMING TOKENS ---
        if app.mode == AppMode::Generating {
            let mut done = false;
            if let Some(rx) = app.token_rx.as_mut() {
                loop {
                    match rx.try_recv() {
                        Ok(token) => {
                            app.pending_response.push_str(&token);
                        }
                        Err(mpsc::error::TryRecvError::Empty) => break,
                        Err(mpsc::error::TryRecvError::Disconnected) => {
                            done = true;
                            break;
                        }
                    }
                }
            }
            if done {
                app.commit_pending_response();
            }
        }

        // --- INPUT EVENTS ---
        let timeout = Duration::from_millis(app.animation.frame_duration_ms());
        if let Some(event) = terminal::poll_event(timeout) {
            match event {
                terminal::AppEvent::Key(key) => {
                    match (key.modifiers, key.code) {
                        (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                            app.should_quit = true;
                        }
                        (KeyModifiers::CONTROL, KeyCode::Char('t')) => {
                            app.cycle_theme();
                        }
                        (KeyModifiers::CONTROL, KeyCode::Char('l')) => {
                            app.chat_messages.clear();
                            app.add_gojo_message("Screen cleared. I'm still here though. Obviously.");
                        }
                        (_, KeyCode::Enter) => {
                            if let Some(msg) = app.submit_input() {
                                if msg.starts_with('/') {
                                    handle_slash_command(&mut app, &msg);
                                } else if app.mode == AppMode::AwaitingConfirmation {
                                    let lower = msg.trim().to_lowercase();
                                    if lower == "yes" || lower == "y" || lower == "do it" || lower == "go" {
                                        if let Some(decision) = app.pending_decision.take() {
                                            let agent_name = decision.agent_needed
                                                .clone()
                                                .unwrap_or_else(|| "claude".to_string());
                                            let task = app.pending_user_message.clone();
                                            let selected_model = decision.select_model(&app.config.agents);
                                            let complexity = format!("{:?}", decision.complexity).to_lowercase();

                                            let agent_type = match agent_name.as_str() {
                                                "codex" => AgentType::Codex {
                                                    model: selected_model.clone(),
                                                },
                                                _ => AgentType::Claude {
                                                    model: selected_model.clone(),
                                                },
                                            };

                                            match orchestrator.spawn(agent_type, &task, None, None).await {
                                                Ok(session) => {
                                                    app.add_gojo_message(&format!(
                                                        "Spawning {} ({} -- {}). Stand back.",
                                                        agent_name, complexity, selected_model
                                                    ));
                                                    app.pending_response.clear();
                                                    app.token_rx = Some(session.output_rx);
                                                    app.mode = AppMode::Generating;
                                                }
                                                Err(e) => {
                                                    app.add_gojo_message(&format!(
                                                        "Couldn't spawn {}: {}", agent_name, e
                                                    ));
                                                    app.mode = AppMode::Normal;
                                                }
                                            }
                                        } else {
                                            app.mode = AppMode::Normal;
                                        }
                                    } else {
                                        // User gave different instructions -- treat as new message
                                        app.mode = AppMode::Normal;
                                        app.pending_decision = None;
                                        dispatch_message(&mut app, &llm_client, &engram_client, &system_prompt, msg);
                                    }
                                } else if app.mode == AppMode::Normal {
                                    dispatch_message(&mut app, &llm_client, &engram_client, &system_prompt, msg);
                                }
                            }
                        }
                        (_, KeyCode::Backspace) => {
                            app.handle_backspace();
                        }
                        (_, KeyCode::Up) => {
                            app.scroll_up();
                        }
                        (_, KeyCode::Down) => {
                            app.scroll_down();
                        }
                        (_, KeyCode::Char(c)) => {
                            app.handle_input_char(c);
                        }
                        _ => {}
                    }
                }
                terminal::AppEvent::Tick => {}
                _ => {}
            }
        }
    }

    terminal::restore()?;
    Ok(())
}

/// Dispatch a user message: fire casual stream immediately + routing in parallel.
/// If routing says memory/action, the casual stream gets aborted and replaced.
fn dispatch_message(
    app: &mut App,
    llm_client: &Arc<LlmClient>,
    _engram_client: &Arc<EngramClient>,
    system_prompt: &str,
    msg: String,
) {
    app.pending_user_message = msg.clone();

    // Start casual stream immediately -- no waiting
    fire_casual_stream(app, llm_client, system_prompt, &msg);

    // Run router in parallel
    let client = llm_client.clone();
    let model = app.config.llm.model_name.clone();
    let temp = app.config.llm.temperature_routing;
    let msg_clone = msg.clone();
    let (tx, rx) = oneshot::channel();

    tokio::spawn(async move {
        let result = RoutingDecision::route(&client, &msg_clone, &model, temp).await;
        let _ = tx.send(result);
    });

    app.routing_rx = Some(rx);
}

/// Fire a casual streaming completion using current conversation history.
fn fire_casual_stream(
    app: &mut App,
    llm_client: &Arc<LlmClient>,
    system_prompt: &str,
    user_msg: &str,
) {
    let history = build_history(app);
    let (tx, rx) = mpsc::unbounded_channel();
    app.start_streaming(rx);

    let client = llm_client.clone();
    let sys = system_prompt.to_string();
    let model = app.config.llm.model_name.clone();
    let temp = app.config.llm.temperature_casual;
    let user_msg = user_msg.to_string();

    let handle = tokio::spawn(async move {
        let mut msgs: Vec<(&str, String)> = vec![("system", sys)];
        for (role, content) in &history {
            msgs.push((role.as_str(), content.clone()));
        }
        // Ensure the current user message is included
        if msgs.last().map(|m| m.0) != Some("user") {
            msgs.push(("user", user_msg));
        }

        let msg_refs: Vec<(&str, &str)> = msgs.iter()
            .map(|(r, c)| (*r, c.as_str()))
            .collect();
        let request = LlmClient::build_request_with_model(&model, &msg_refs, temp, None);
        let _ = client.stream_complete(&request, tx).await;
    });
    app.stream_abort = Some(handle.abort_handle());
}

/// Build conversation history from chat messages (excludes the most recent user message,
/// which is handled separately by each dispatch path).
fn build_history(app: &App) -> Vec<(String, String)> {
    app.chat_messages.iter()
        .map(|m| {
            let role = if m.is_user { "user".to_string() } else { "assistant".to_string() };
            (role, m.content.clone())
        })
        .collect()
}

fn handle_slash_command(app: &mut App, msg: &str) {
    let parts: Vec<&str> = msg.split_whitespace().collect();
    let cmd = parts.first().map(|s| *s).unwrap_or("");

    match cmd {
        "/theme" => {
            if let Some(name) = parts.get(1) {
                if eidolon_tui::tui::theme::Theme::by_name(name).is_some() {
                    app.cycle_theme();
                    app.add_gojo_message("Theme switched. Looking good.");
                } else {
                    app.add_gojo_message(&format!("No theme called '{}'. Nice try.", name));
                }
            } else {
                app.add_gojo_message(&format!("Current theme: {}", app.theme.name));
            }
        }
        "/model" | "/models" => {
            if parts.len() == 1 {
                let c = &app.config.agents.claude;
                let x = &app.config.agents.codex;
                app.add_gojo_message(&format!(
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
                            app.add_gojo_message(&format!("{} light -> {}", agent, model));
                        }
                        "medium" => {
                            entry.model_medium = model.clone();
                            app.add_gojo_message(&format!("{} medium -> {}", agent, model));
                        }
                        "heavy" => {
                            entry.model_heavy = model.clone();
                            app.add_gojo_message(&format!("{} heavy -> {}", agent, model));
                        }
                        _ => {
                            app.add_gojo_message("Tiers: light, medium, heavy");
                        }
                    }
                } else {
                    app.add_gojo_message("Agents: claude, codex");
                }
            } else {
                app.add_gojo_message("Usage: /model <claude|codex> <light|medium|heavy> <model>");
            }
        }
        "/status" => {
            let sidecar = format!("{:?}", app.sidecar_status);
            let c = &app.config.agents.claude;
            app.add_gojo_message(&format!(
                "LLM: {}\nEngram: {}\nModels: light={}, medium={}, heavy={}",
                sidecar, app.config.engram.url,
                c.model_light, c.model_medium, c.model_heavy
            ));
        }
        "/help" => {
            app.add_gojo_message(
                "Commands:\n  /model           - show/set model tiers\n  /status          - system status\n  /theme <name>    - switch theme\n  /clear           - clear chat (or Ctrl+L)\n  /quit            - exit"
            );
        }
        "/clear" => {
            app.chat_messages.clear();
            app.add_gojo_message("Screen cleared. I'm still here though. Obviously.");
        }
        "/quit" | "/exit" => {
            app.should_quit = true;
        }
        _ => {
            app.add_gojo_message(&format!("Unknown command: {}. Try /help", cmd));
        }
    }
}

/// Resolve short model aliases to full model IDs.
fn resolve_model_alias(input: &str) -> String {
    match input.to_lowercase().as_str() {
        // Claude
        "haiku" => "claude-haiku-4-5-20251001".to_string(),
        "sonnet" => "claude-sonnet-4-6".to_string(),
        "opus" => "claude-opus-4-6".to_string(),
        // Codex / OpenAI
        "5.4" => "gpt-5.4".to_string(),
        "5.4-mini" => "gpt-5.4-mini".to_string(),
        "5.3-codex" => "gpt-5.3-codex".to_string(),
        "5.2-codex" => "gpt-5.2-codex".to_string(),
        "5.2" => "gpt-5.2".to_string(),
        "5.1-max" => "gpt-5.1-codex-max".to_string(),
        "5.1-mini" => "gpt-5.1-codex-mini".to_string(),
        // Pass through anything else as-is
        other => other.to_string(),
    }
}
