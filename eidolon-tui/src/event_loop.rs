use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

use crate::app::{App, AppMode, ServiceState};
use crate::commands;
use crate::config::Config;
use crate::conversation::personality::gojo_system_prompt;
use crate::conversation::router;
use crate::daemon::client::DaemonClient;
use crate::dispatch;
use crate::intelligence::pipeline::Pipeline;
use crate::intelligence::synthesizer::Synthesizer;
use crate::llm::client::LlmClient;
use crate::llm::sidecar::{LlamaSidecar, SidecarStatus};
use crate::syntheos::engram::EngramClient;
use crate::tui::terminal;
use crate::tui::widgets::{
    agent_panel::AgentPanel,
    chat_area::ChatArea,
    input_bar::InputBar,
    status_bar::StatusBar,
    thinking::ThinkingSpinner,
};

pub async fn run(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    // Panic hook: restore terminal and cleanup sidecar
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = terminal::restore();
        LlamaSidecar::cleanup_by_pid_file();
        default_hook(info);
    }));

    let mut tui = terminal::init()?;
    let system_prompt = gojo_system_prompt();
    let (system_msg_tx, system_msg_rx) = mpsc::unbounded_channel::<String>();
    let mut app = App::new(config.clone(), system_prompt, system_msg_rx);

    // --- CONNECT DAEMON (optional) ---
    let mut daemon_client: Option<Arc<DaemonClient>> = if !config.daemon.api_key.is_empty() {
        let client = DaemonClient::new(&config.daemon.url, &config.daemon.api_key);
        match client.health().await {
            Ok(()) => {
                app.daemon_state = ServiceState::Connected;
                Some(Arc::new(client))
            }
            Err(_) => {
                app.daemon_state = ServiceState::Disconnected;
                None
            }
        }
    } else {
        app.daemon_state = ServiceState::Unavailable;
        None
    };

    // --- INIT ENGRAM (optional -- no more process::exit!) ---
    let engram_client: Option<Arc<EngramClient>> = match EngramClient::new(
        &app.config.engram.url,
        &app.config.engram.api_key,
    ) {
        Ok(client) => {
            app.engram_state = ServiceState::Connected;
            Some(Arc::new(client))
        }
        Err(_) => {
            app.engram_state = ServiceState::Disconnected;
            None
        }
    };

    let llm_base_url = app.config.llm.base_url.clone()
        .unwrap_or_else(|| format!("http://localhost:{}", app.config.llm.port));
    let mut llm_client = Arc::new(LlmClient::new(&llm_base_url));

    // --- SYNTHEOS REGISTRATION (fire-and-forget) ---
    {
        let engram_url = app.config.engram.url.clone();
        let engram_key = app.config.engram.api_key.clone();
        tokio::spawn(async move {
            let http = reqwest::Client::new();
            let auth = format!("Bearer {}", engram_key);

            // Register with Soma
            let _ = http.post(format!("{}/soma/agents", engram_url))
                .header("Authorization", &auth)
                .json(&serde_json::json!({
                    "name": "eidolon-tui",
                    "type": "interactive",
                    "description": "Eidolon TUI agent orchestrator",
                    "capabilities": ["conversation", "agent-spawn", "memory-search"]
                }))
                .send().await;

            // Publish agent.online to Axon
            let _ = http.post(format!("{}/axon/publish", engram_url))
                .header("Authorization", &auth)
                .json(&serde_json::json!({
                    "channel": "system",
                    "source": "eidolon-tui",
                    "type": "agent.online",
                    "payload": {"agent": "eidolon-tui", "task": "interactive session"}
                }))
                .send().await;

            // Create Chiasm task
            let _ = http.post(format!("{}/tasks", engram_url))
                .header("Authorization", &auth)
                .json(&serde_json::json!({
                    "agent": "eidolon-tui",
                    "project": "eidolon",
                    "title": "Interactive TUI session"
                }))
                .send().await;
        });
    }

    // --- SIDECAR LIFECYCLE ---
    // Sidecar is created in a spawned task and sent back via channel.
    // Once received, it lives as Option<LlamaSidecar> on the main thread.
    let mut sidecar: Option<LlamaSidecar> = None;
    let mut sidecar_startup_rx: Option<oneshot::Receiver<LlamaSidecar>> = None;

    if app.config.llm.base_url.is_none() && !app.config.llm.model_path.is_empty() {
        let server_path = app.config.llm.server_path.clone();
        let model_path = app.config.llm.model_path.clone();
        let port = app.config.llm.port;
        let ctx = app.config.llm.context_length;
        let ngl = app.config.llm.gpu_layers;
        let (tx, rx) = oneshot::channel();
        sidecar_startup_rx = Some(rx);
        app.sidecar_status = SidecarStatus::Starting;

        tokio::spawn(async move {
            let mut sc = LlamaSidecar::new(&server_path, &model_path, port, ctx, ngl);
            let _ = sc.start().await;
            let _ = tx.send(sc);
        });
    } else if app.config.llm.base_url.is_some() {
        app.sidecar_status = SidecarStatus::Ready;
    }

    // Health check state for periodic polling
    let mut health_check_rx: Option<oneshot::Receiver<bool>> = None;
    let mut last_health_check = std::time::Instant::now();

    // --- MAIN LOOP ---
    loop {
        if app.should_quit {
            break;
        }

        // --- POLL SIDECAR STARTUP ---
        if let Some(rx) = sidecar_startup_rx.as_mut() {
            match rx.try_recv() {
                Ok(sc) => {
                    let status = sc.status().clone();
                    let actual_port = sc.port();
                    if let SidecarStatus::Error(ref e) = status {
                        app.add_system_message(&format!("LLM failed to start: {}", e));
                    }
                    if status == SidecarStatus::Ready && actual_port != app.config.llm.port {
                        let new_url = format!("http://localhost:{}", actual_port);
                        llm_client = Arc::new(LlmClient::new(&new_url));
                        app.config.llm.port = actual_port;
                    }
                    app.sidecar_status = status;
                    sidecar = Some(sc);
                    sidecar_startup_rx = None;
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
                Err(oneshot::error::TryRecvError::Closed) => {
                    app.sidecar_status = SidecarStatus::Error("Sidecar task crashed".to_string());
                    sidecar_startup_rx = None;
                }
            }
        }

        // --- POLL LLM HEALTH (periodic, non-blocking) ---
        if matches!(app.sidecar_status, SidecarStatus::Ready | SidecarStatus::Degraded(_)) {
            if let Some(rx) = health_check_rx.as_mut() {
                match rx.try_recv() {
                    Ok(true) => {
                        if matches!(app.sidecar_status, SidecarStatus::Degraded(_)) {
                            app.sidecar_status = SidecarStatus::Ready;
                        }
                        health_check_rx = None;
                    }
                    Ok(false) => {
                        if app.sidecar_status == SidecarStatus::Ready {
                            app.sidecar_status = SidecarStatus::Degraded("health check failed".to_string());
                        }
                        health_check_rx = None;
                    }
                    Err(oneshot::error::TryRecvError::Empty) => {}
                    Err(oneshot::error::TryRecvError::Closed) => {
                        health_check_rx = None;
                    }
                }
            }

            // Kick off a new check every 5s if none in flight
            if health_check_rx.is_none() {
                let now = std::time::Instant::now();
                if now.duration_since(last_health_check) >= Duration::from_secs(5) {
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
        let input_height = (app.input.lines().count().max(1) + 2).min(8) as u16;
        tui.draw(|frame| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Min(5),
                    Constraint::Length(input_height),
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

            let mut display_messages = app.display_messages();
            if (app.mode == AppMode::Generating || app.mode == AppMode::Routing)
                && !app.pending_response.is_empty()
            {
                display_messages.push(crate::tui::widgets::chat_area::ChatMessage {
                    sender: "Gojo".to_string(),
                    content: app.pending_response.clone(),
                    is_user: false,
                });
            }
            // Split chat area horizontally if agent panel is visible
            let (chat_area, panel_area) = if app.show_agent_panel {
                let h_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
                    .split(chunks[1]);
                (h_chunks[0], Some(h_chunks[1]))
            } else {
                (chunks[1], None)
            };

            let chat = ChatArea::new(app.theme, &display_messages, app.scroll_offset);
            frame.render_widget(chat, chat_area);

            // Render agent panel if toggled
            if let Some(panel_rect) = panel_area {
                // Show active agent sessions or empty state
                if let Some((session_id, lines)) = app.agent_outputs.iter().next() {
                    let agent_panel = AgentPanel::new(
                        app.theme,
                        &app.animation,
                        "agent",
                        session_id,
                        0,
                        lines,
                        matches!(app.mode, AppMode::Generating | AppMode::AgentActive { .. }),
                    );
                    frame.render_widget(agent_panel, panel_rect);
                } else {
                    // Empty state
                    let empty = ratatui::widgets::Paragraph::new("No active sessions")
                        .style(ratatui::style::Style::default().fg(app.theme.dim))
                        .block(
                            ratatui::widgets::Block::default()
                                .borders(ratatui::widgets::Borders::ALL)
                                .border_style(ratatui::style::Style::default().fg(app.theme.dim))
                                .title(ratatui::text::Span::styled(
                                    " Agents ",
                                    ratatui::style::Style::default().fg(app.theme.accent),
                                ))
                                .style(ratatui::style::Style::default().bg(app.theme.bg_secondary)),
                        );
                    frame.render_widget(empty, panel_rect);
                }
            }

            if app.mode == AppMode::Generating || app.mode == AppMode::Routing {
                let spinner = ThinkingSpinner::new(app.theme, &app.animation);
                let spinner_area = ratatui::layout::Rect {
                    x: chat_area.x + 2,
                    y: chat_area.bottom().saturating_sub(1),
                    width: chat_area.width.saturating_sub(4),
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
                                router::Intent::Casual => {
                                    app.pending_decision = Some(decision);
                                }
                                router::Intent::Memory => {
                                    if let Some(abort) = app.stream_abort.take() {
                                        abort.abort();
                                    }
                                    app.pending_response.clear();
                                    app.pending_decision = Some(decision);
                                    if let Some(ref engram) = engram_client {
                                        dispatch::fire_memory_stream(
                                            &mut app,
                                            &llm_client,
                                            engram,
                                            &user_msg,
                                        );
                                    } else {
                                        // Engram unavailable -- fall back to casual stream
                                        dispatch::fire_casual_stream(&mut app, &llm_client, &user_msg);
                                    }
                                }
                                router::Intent::Action => {
                                    if let Some(abort) = app.stream_abort.take() {
                                        abort.abort();
                                    }
                                    app.pending_response.clear();
                                    app.token_rx = None;

                                    if matches!(app.sidecar_status, SidecarStatus::Ready) {
                                        // Run intelligence pipeline (compress -> distill -> select)
                                        app.pending_decision = Some(decision.clone());
                                        app.mode = AppMode::Optimizing;
                                        app.add_system_message("Optimizing prompt...");

                                        let client = llm_client.clone();
                                        let model_name = app.config.llm.model_name.clone();
                                        let history = app.conversation.build_api_messages();
                                        let umsg = user_msg.clone();
                                        let agents_cfg = app.config.agents.clone();
                                        let engram = engram_client.clone();
                                        let dec = decision;

                                        let (tx, rx) = oneshot::channel();
                                        app.pipeline_rx = Some(rx);

                                        tokio::spawn(async move {
                                            let mut pipeline = Pipeline::new(client, &model_name);
                                            let result = pipeline.run(
                                                &umsg, &history, &dec, &agents_cfg, &engram,
                                            ).await;
                                            let _ = tx.send(result);
                                        });
                                    } else {
                                        // Sidecar unavailable -- skip pipeline
                                        let agent = decision.agent_needed.clone()
                                            .unwrap_or_else(|| "claude".to_string());
                                        let model = decision.select_model(&app.config.agents);
                                        let complexity = format!("{:?}", decision.complexity).to_lowercase();
                                        let reasoning = decision.reasoning.clone();
                                        app.pending_decision = Some(decision);
                                        app.mode = AppMode::AwaitingConfirmation;
                                        app.add_system_message(&format!(
                                            "I'd use {} for this ({} complexity -> {}). {}\n\nSay yes to proceed, or tell me what to do differently.",
                                            agent, complexity, model, reasoning
                                        ));
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            let fallback = crate::conversation::router::RoutingDecision::keyword_fallback(&app.pending_user_message);
                            match fallback.intent {
                                router::Intent::Action => {
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
                                    app.add_system_message(&format!(
                                        "Router hiccup ({}), but this looks like an action. Use {} (-> {})?\n\nSay yes to proceed.",
                                        e, agent, model
                                    ));
                                }
                                _ => {
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

        // --- POLL PIPELINE RESULT ---
        if app.mode == AppMode::Optimizing {
            if let Some(rx) = app.pipeline_rx.as_mut() {
                match rx.try_recv() {
                    Ok(result) => {
                        app.pipeline_rx = None;
                        let approval_text = result.format_for_approval();
                        app.pending_pipeline_result = Some(result);
                        app.mode = AppMode::AwaitingConfirmation;
                        app.add_system_message(&approval_text);
                    }
                    Err(oneshot::error::TryRecvError::Empty) => {}
                    Err(oneshot::error::TryRecvError::Closed) => {
                        // Pipeline task crashed -- fall back to simple confirmation
                        app.pipeline_rx = None;
                        app.mode = AppMode::AwaitingConfirmation;
                        if let Some(ref decision) = app.pending_decision {
                            let agent = decision.agent_needed.clone()
                                .unwrap_or_else(|| "claude".to_string());
                            let model = decision.select_model(&app.config.agents);
                            app.add_system_message(&format!(
                                "Pipeline failed. Use {} ({})?\n\nSay yes to proceed.",
                                agent, model
                            ));
                        }
                    }
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
                // If this was a daemon action response and sidecar is available, synthesize
                let is_action = app.pending_decision.as_ref()
                    .map(|d| matches!(d.intent, router::Intent::Action))
                    .unwrap_or(false);
                if is_action && matches!(app.sidecar_status, SidecarStatus::Ready) && !app.pending_response.is_empty() {
                    let raw = app.pending_response.clone();
                    let client = llm_client.clone();
                    let model = app.config.llm.model_name.clone();
                    let (tx, rx) = oneshot::channel();
                    app.synthesis_rx = Some(rx);
                    tokio::spawn(async move {
                        let synth = Synthesizer::new(client, &model);
                        let result = synth.synthesize(&raw).await;
                        let _ = tx.send(result);
                    });
                }
                app.commit_pending_response();
            }
        }

        // --- POLL SYNTHESIS RESULT ---
        if let Some(rx) = app.synthesis_rx.as_mut() {
            match rx.try_recv() {
                Ok(result) => {
                    app.synthesis_rx = None;
                    let display = result.format_for_display();
                    app.last_synthesized = Some(result);
                    app.add_system_message(&format!("[Synthesis]\n{}\n(Ctrl+E to toggle raw output)", display));
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
                Err(oneshot::error::TryRecvError::Closed) => {
                    app.synthesis_rx = None;
                }
            }
        }

        // --- DRAIN SYSTEM MESSAGES ---
        while let Ok(msg) = app.system_msg_rx.try_recv() {
            app.add_system_message(&msg);
        }

        // --- POLL DAEMON RECONNECT ---
        if let Some(rx) = app.daemon_reconnect_rx.as_mut() {
            match rx.try_recv() {
                Ok(Ok(client)) => {
                    daemon_client = Some(Arc::new(client));
                    app.daemon_state = ServiceState::Connected;
                    app.add_system_message("Daemon connected.");
                    app.daemon_reconnect_rx = None;
                }
                Ok(Err(e)) => {
                    app.add_system_message(&format!("Daemon reconnect failed: {}", e));
                    app.daemon_reconnect_rx = None;
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
                Err(oneshot::error::TryRecvError::Closed) => {
                    app.daemon_reconnect_rx = None;
                }
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
                            app.clear_messages();
                            app.add_system_message("Screen cleared. I'm still here though. Obviously.");
                        }
                        (KeyModifiers::CONTROL, KeyCode::Char('e')) => {
                            app.show_raw_output = !app.show_raw_output;
                            if let Some(ref synth) = app.last_synthesized {
                                if app.show_raw_output {
                                    app.add_system_message(&format!("[Raw output]\n{}", synth.raw_output));
                                } else {
                                    app.add_system_message(&format!("[Synthesis]\n{}", synth.format_for_display()));
                                }
                            }
                        }
                        (KeyModifiers::CONTROL, KeyCode::Char('a')) => {
                            app.show_agent_panel = !app.show_agent_panel;
                        }
                        (KeyModifiers::SHIFT, KeyCode::Enter) => {
                            app.handle_input_char('\n');
                        }
                        (_, KeyCode::Enter) => {
                            if let Some(msg) = app.submit_input() {
                                if msg.starts_with('/') {
                                    commands::handle_command(&mut app, &msg, &daemon_client, &system_msg_tx);
                                } else if app.mode == AppMode::AwaitingConfirmation {
                                    handle_confirmation(&mut app, &msg, &daemon_client);
                                } else if app.mode == AppMode::Normal {
                                    dispatch::dispatch_message(&mut app, &llm_client, &engram_client, msg);
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
                        (_, KeyCode::PageUp) => {
                            app.scroll_page_up(20);
                        }
                        (_, KeyCode::PageDown) => {
                            app.scroll_page_down(20);
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

    // --- SHUTDOWN ---
    // Sidecar cleanup happens via Drop when `sidecar` goes out of scope
    drop(sidecar);

    // Syntheos shutdown (fire-and-forget)
    {
        let engram_url = config.engram.url.clone();
        let engram_key = config.engram.api_key.clone();
        let _ = tokio::spawn(async move {
            let http = reqwest::Client::new();
            let auth = format!("Bearer {}", engram_key);
            let _ = http.post(format!("{}/axon/publish", engram_url))
                .header("Authorization", &auth)
                .json(&serde_json::json!({
                    "channel": "system",
                    "source": "eidolon-tui",
                    "type": "agent.offline",
                    "payload": {"agent": "eidolon-tui", "summary": "TUI session ended"}
                }))
                .send().await;
        }).await;
    }

    terminal::restore()?;
    Ok(())
}

/// Handle user response to an action confirmation prompt.
fn handle_confirmation(
    app: &mut App,
    msg: &str,
    daemon_client: &Option<Arc<DaemonClient>>,
) {
    let lower = msg.trim().to_lowercase();
    if lower == "yes" || lower == "y" || lower == "do it" || lower == "go" {
        if let Some(decision) = app.pending_decision.take() {
            let agent_name = decision.agent_needed
                .clone()
                .unwrap_or_else(|| "claude".to_string());
            // Use distilled prompt from pipeline if available, otherwise raw user message
            let task = app.pending_pipeline_result.as_ref()
                .map(|p| p.distilled.format_for_agent())
                .unwrap_or_else(|| app.pending_user_message.clone());
            let selected_model = app.pending_pipeline_result.as_ref()
                .map(|p| p.selection.model.clone())
                .unwrap_or_else(|| decision.select_model(&app.config.agents));
            let complexity = format!("{:?}", decision.complexity).to_lowercase();

            if let Some(ref daemon) = daemon_client {
                let daemon = Arc::clone(daemon);
                let task_clone = task;
                let agent_clone = agent_name.clone();
                let model_clone = selected_model.clone();

                app.add_system_message(&format!(
                    "Spawning {} ({} - {}) via daemon. Stand back.",
                    agent_clone, complexity, model_clone
                ));

                let (tx, rx) = mpsc::unbounded_channel();
                app.pending_response.clear();
                app.token_rx = Some(rx);
                app.mode = AppMode::Generating;

                tokio::spawn(async move {
                    // Try to enrich task via daemon prompt generation
                    let final_task = match daemon.generate_prompt(&task_clone, &agent_clone).await {
                        Ok(enriched) => enriched,
                        Err(_) => task_clone, // Fall back to distilled prompt as-is
                    };

                    match daemon.submit_task(&final_task, &agent_clone, &model_clone).await {
                        Ok(session) => {
                            let _ = tx.send(format!(
                                "[Daemon session {} started]\n",
                                session.session_id
                            ));
                            if let Err(e) = daemon.stream_session(&session.session_id, tx.clone()).await {
                                let _ = tx.send(format!("\n[Stream error: {}]", e));
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(format!("[Daemon error: {}]", e));
                        }
                    }
                });
            } else {
                app.add_system_message(
                    "Daemon not connected -- cannot spawn agents. Configure [daemon] in config.toml with url and api_key."
                );
                app.mode = AppMode::Normal;
            }
        } else {
            app.mode = AppMode::Normal;
        }
    } else {
        // User gave different instructions -- treat as new message
        app.mode = AppMode::Normal;
        app.pending_decision = None;
        app.pending_pipeline_result = None;
        // Note: message was already added to conversation by submit_input()
        // Re-dispatch would double-add it. Just reset to normal mode.
    }
}
