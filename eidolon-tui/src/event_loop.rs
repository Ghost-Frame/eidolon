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
use crate::embedding::AsyncEmbeddingProvider;
use crate::syntheos::engram::EngramClient;
use crate::tui::terminal;
use crate::app::InputTarget;
use crate::app::ClaudeSessionState;
use crate::tui::widgets::{
    chat_area::ChatArea,
    claude_panel::ClaudePanel,
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

    // --- INIT EMBEDDING PROVIDER (optional, config-driven) ---
    let embed_provider: Option<Arc<dyn AsyncEmbeddingProvider>> =
        crate::config::build_embed_provider(&config);

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
        let elapsed_secs = match &app.claude_session.state {
            ClaudeSessionState::Active { started_at, .. } => started_at.elapsed().as_secs(),
            _ => app.claude_session.started_at.map(|t| t.elapsed().as_secs()).unwrap_or(0),
        };
        let split_pct = app.panel_split_percent;
        tui.draw(|frame| {
            // Fill entire frame with theme background
            frame.render_widget(
                ratatui::widgets::Block::default()
                    .style(ratatui::style::Style::default().bg(app.theme.bg)),
                frame.area(),
            );
            // Vertical: [status_bar (1)] [panels (flex)] [input_bar]
            let v_chunks = Layout::default()
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
                app.input_target,
                &app.claude_session.state,
            );
            frame.render_widget(status, v_chunks[0]);

            // Horizontal: [left: TUI chat (split%)] [right: Claude panel (100-split%)]
            let panel_area = v_chunks[1];
            let h_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(split_pct),
                    Constraint::Percentage(100 - split_pct),
                ])
                .split(panel_area);

            let left_rect = h_chunks[0];
            let right_rect = h_chunks[1];

            // Store rects and divider col for mouse hit testing (captured via closure refs)
            // We use local vars here; the app fields are updated after draw via separate mutation
            let divider = left_rect.x + left_rect.width;

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

            let chat = ChatArea::new(
                app.theme,
                &display_messages,
                app.tui_scroll_offset,
                app.input_target == InputTarget::Tui,
            );
            frame.render_widget(chat, left_rect);

            // Thinking spinner on left panel only
            if app.mode == AppMode::Generating || app.mode == AppMode::Routing {
                let spinner = ThinkingSpinner::new(app.theme, &app.animation);
                let spinner_area = ratatui::layout::Rect {
                    x: left_rect.x + 2,
                    y: left_rect.bottom().saturating_sub(1),
                    width: left_rect.width.saturating_sub(4),
                    height: 1,
                };
                frame.render_widget(spinner, spinner_area);
            }

            let claude = ClaudePanel::new(
                app.theme,
                &app.animation,
                &app.claude_session,
                app.input_target == InputTarget::Claude,
                elapsed_secs,
            );
            frame.render_widget(claude, right_rect);

            let input = InputBar::new(app.theme, &app.input, app.cursor_pos, app.input_target);
            frame.render_widget(input, v_chunks[2]);

            // Store layout rects for mouse hit testing -- use a thread_local trick via
            // a raw pointer write. Since draw closure borrows app immutably we store via
            // the captured locals after draw completes.
            let _ = (left_rect, right_rect, divider); // keep them alive in closure
        })?;

        // Update stored rects after draw (borrow released)
        {
            let sz = tui.size()?;
            let frame_area = ratatui::layout::Rect {
                x: 0,
                y: 0,
                width: sz.width,
                height: sz.height,
            };
            let v_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Min(5),
                    Constraint::Length(input_height),
                ])
                .split(frame_area);
            let panel_area = v_chunks[1];
            let h_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(split_pct),
                    Constraint::Percentage(100 - split_pct),
                ])
                .split(panel_area);
            app.left_panel_rect = h_chunks[0];
            app.right_panel_rect = h_chunks[1];
            app.divider_col = h_chunks[0].x + h_chunks[0].width;
        }

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

                                    app.pending_decision = Some(decision.clone());

                                    if let Some(ref daemon) = daemon_client {
                                        // Daemon connected -- delegate prompt generation to daemon
                                        app.mode = AppMode::Optimizing;
                                        app.add_system_message("Generating prompt via daemon...");

                                        let daemon = Arc::clone(daemon);
                                        let umsg = user_msg.clone();
                                        let (tx, rx) = oneshot::channel();
                                        app.daemon_prompt_rx = Some(rx);

                                        tokio::spawn(async move {
                                            let result = daemon.generate_prompt(&umsg, "claude").await;
                                            let _ = tx.send(result);
                                        });
                                    } else if matches!(app.sidecar_status, SidecarStatus::Ready) {
                                        // No daemon -- fall back to local pipeline (compress -> distill -> select)
                                        app.mode = AppMode::Optimizing;
                                        app.add_system_message("Optimizing prompt...");

                                        let client = llm_client.clone();
                                        let model_name = app.config.llm.model_name.clone();
                                        let history = app.conversation.build_api_messages();
                                        let umsg = user_msg.clone();
                                        let agents_cfg = app.config.agents.clone();
                                        let engram = engram_client.clone();
                                        let embed = embed_provider.clone();
                                        let dec = decision;

                                        let (tx, rx) = oneshot::channel();
                                        app.pipeline_rx = Some(rx);

                                        tokio::spawn(async move {
                                            let mut pipeline = Pipeline::new(client, &model_name);
                                            let result = pipeline.run(
                                                &umsg, &history, &dec, &agents_cfg, &engram, &embed,
                                            ).await;
                                            let _ = tx.send(result);
                                        });
                                    } else {
                                        // Neither daemon nor sidecar available -- skip optimization
                                        let agent = decision.agent_needed.clone()
                                            .unwrap_or_else(|| "claude".to_string());
                                        let model = decision.select_model(&app.config.agents);
                                        let complexity = format!("{:?}", decision.complexity).to_lowercase();
                                        let reasoning = decision.reasoning.clone();
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

        // --- POLL DAEMON PROMPT RESULT ---
        if app.mode == AppMode::Optimizing {
            if let Some(rx) = app.daemon_prompt_rx.as_mut() {
                match rx.try_recv() {
                    Ok(result) => {
                        app.daemon_prompt_rx = None;
                        match result {
                            Ok(prompt) => {
                                // Show the daemon-generated prompt in Claude panel for preview
                                app.claude_session.messages.push(
                                    format!("--- Pending dispatch (daemon prompt) ---\n{}", prompt)
                                );
                                app.pending_daemon_prompt = Some(prompt.clone());

                                // Show approval in left panel with agent/model info
                                let agent = app.pending_decision.as_ref()
                                    .and_then(|d| d.agent_needed.clone())
                                    .unwrap_or_else(|| "claude".to_string());
                                let model = app.pending_decision.as_ref()
                                    .map(|d| d.select_model(&app.config.agents))
                                    .unwrap_or_else(|| "unknown".to_string());
                                let complexity = app.pending_decision.as_ref()
                                    .map(|d| format!("{:?}", d.complexity).to_lowercase())
                                    .unwrap_or_else(|| "medium".to_string());
                                app.mode = AppMode::AwaitingConfirmation;
                                app.add_system_message(&format!(
                                    "Agent: {} ({})\nComplexity: {}\n\nPrompt preview is in the right panel.\n\nSay yes to proceed, or tell me what to change.",
                                    agent, model, complexity
                                ));
                            }
                            Err(e) => {
                                // Daemon prompt failed -- show simple confirmation without preview
                                app.pending_daemon_prompt = None;
                                let agent = app.pending_decision.as_ref()
                                    .and_then(|d| d.agent_needed.clone())
                                    .unwrap_or_else(|| "claude".to_string());
                                let model = app.pending_decision.as_ref()
                                    .map(|d| d.select_model(&app.config.agents))
                                    .unwrap_or_else(|| "unknown".to_string());
                                app.mode = AppMode::AwaitingConfirmation;
                                app.add_system_message(&format!(
                                    "Daemon prompt generation failed: {}\n\nUse {} ({}) with original message?\n\nSay yes to proceed.",
                                    e, agent, model
                                ));
                            }
                        }
                    }
                    Err(oneshot::error::TryRecvError::Empty) => {}
                    Err(oneshot::error::TryRecvError::Closed) => {
                        app.daemon_prompt_rx = None;
                        app.mode = AppMode::AwaitingConfirmation;
                        if let Some(ref decision) = app.pending_decision {
                            let agent = decision.agent_needed.clone()
                                .unwrap_or_else(|| "claude".to_string());
                            let model = decision.select_model(&app.config.agents);
                            app.add_system_message(&format!(
                                "Daemon prompt task crashed. Use {} ({}) with original message?\n\nSay yes to proceed.",
                                agent, model
                            ));
                        }
                    }
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
                        // Show preview in Claude panel
                        let preview = format!("--- Pending dispatch ---\n{}", result.distilled.format_for_display());
                        app.claude_session.messages.push(preview);
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

        // --- DRAIN CLAUDE OUTPUT ---
        if let Some(rx) = app.claude_output_rx.as_mut() {
            let mut done = false;
            let mut session_ended = false;
            let mut session_exit_code: i32 = 0;
            let mut session_failed: Option<String> = None;

            loop {
                match rx.try_recv() {
                    Ok(line) => {
                        // Parse lifecycle messages
                        if line.contains("[Session ended:") {
                            // Extract exit code from "[Session ended: <status> (exit code <n>)]"
                            let exit_code = line
                                .find("exit code ")
                                .and_then(|i| {
                                    line[i + 10..]
                                        .trim_end_matches(']')
                                        .trim()
                                        .parse::<i32>()
                                        .ok()
                                })
                                .unwrap_or(0);
                            session_exit_code = exit_code;
                            session_ended = true;
                            app.claude_session.messages.push(line);
                        } else if line.contains("[error]") || line.contains("[WebSocket error:") {
                            session_failed = Some(line.clone());
                            app.claude_session.messages.push(line);
                        } else {
                            app.claude_session.messages.push(line);
                        }
                    }
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        done = true;
                        break;
                    }
                }
            }

            if session_ended || done || session_failed.is_some() {
                if let Some(error_msg) = session_failed {
                    app.claude_session.state = ClaudeSessionState::Failed {
                        error: error_msg,
                    };
                    app.add_system_message("Claude session failed. Check right panel.");
                } else if session_ended || done {
                    if let ClaudeSessionState::Active { ref session_id, .. } =
                        app.claude_session.state.clone()
                    {
                        let sid = session_id.clone();
                        app.claude_session.state = ClaudeSessionState::Completed {
                            session_id: sid,
                            exit_code: session_exit_code,
                        };
                    }
                    app.add_system_message("Claude finished. Check right panel.");
                }
                // Clear the channel on done, session_ended, OR failure (not just the first two)
                app.claude_output_rx = None;
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
        let mouse_enabled = app.config.tui.mouse_enabled;
        if let Some(event) = terminal::poll_event(timeout, mouse_enabled) {
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
                        (KeyModifiers::CONTROL, KeyCode::Char('y')) => {
                            let text = match app.input_target {
                                InputTarget::Tui => app.last_tui_assistant_message(),
                                InputTarget::Claude => app.last_claude_message(),
                            };
                            match text {
                                Some(t) if !t.is_empty() => {
                                    match arboard::Clipboard::new() {
                                        Ok(mut cb) => {
                                            match cb.set_text(&t) {
                                                Ok(_) => app.add_system_message("Copied to clipboard."),
                                                Err(e) => app.add_system_message(&format!("Clipboard error: {}", e)),
                                            }
                                        }
                                        Err(e) => app.add_system_message(&format!("Clipboard unavailable: {}", e)),
                                    }
                                }
                                _ => app.add_system_message("Nothing to copy."),
                            }
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
                        (_, KeyCode::Tab) => {
                            app.input_target = match app.input_target {
                                InputTarget::Tui => InputTarget::Claude,
                                InputTarget::Claude => InputTarget::Tui,
                            };
                        }
                        (KeyModifiers::SHIFT, KeyCode::Enter) => {
                            app.handle_input_char('\n');
                        }
                        (_, KeyCode::Enter) => {
                            match app.input_target {
                                InputTarget::Tui => {
                                    if let Some(msg) = app.submit_input() {
                                        if msg.starts_with('/') {
                                            commands::handle_command(&mut app, &msg, &daemon_client, &system_msg_tx);
                                        } else if app.mode == AppMode::AwaitingConfirmation {
                                            handle_confirmation(&mut app, &msg, &daemon_client);
                                        } else if app.mode == AppMode::Normal {
                                            dispatch::dispatch_message(&mut app, &llm_client, &engram_client, &embed_provider, msg);
                                        }
                                    }
                                }
                                InputTarget::Claude => {
                                    if !app.input.is_empty() {
                                        let msg = app.input.clone();
                                        app.input.clear();
                                        app.cursor_pos = 0;
                                        app.claude_session.messages.push(format!("> You: {}", msg));
                                        if let ClaudeSessionState::Active { ref session_id, .. } = app.claude_session.state {
                                            if let Some(ref daemon) = daemon_client {
                                                let daemon = Arc::clone(daemon);
                                                let sid = session_id.clone();
                                                let input = msg;
                                                let sys_tx = system_msg_tx.clone();
                                                tokio::spawn(async move {
                                                    if let Err(e) = daemon.send_input(&sid, &input).await {
                                                        let _ = sys_tx.send(format!("Failed to send to Claude: {}", e));
                                                    }
                                                });
                                            } else {
                                                app.claude_session.messages.push("[No daemon connected]".to_string());
                                            }
                                        } else {
                                            app.claude_session.messages.push("[No active session]".to_string());
                                        }
                                    }
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
                terminal::AppEvent::Mouse(mouse) => {
                    use crossterm::event::{MouseButton, MouseEventKind};
                    match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            let col = mouse.column;
                            let row = mouse.row;
                            // Divider drag detection (+/- 1 column)
                            if col >= app.divider_col.saturating_sub(1)
                                && col <= app.divider_col + 1
                                && row >= app.left_panel_rect.top()
                                && row < app.left_panel_rect.bottom()
                            {
                                app.dragging_divider = true;
                            }
                            // Click left panel
                            else if col < app.divider_col
                                && row >= app.left_panel_rect.top()
                                && row < app.left_panel_rect.bottom()
                            {
                                app.input_target = InputTarget::Tui;
                            }
                            // Click right panel
                            else if col >= app.divider_col
                                && row >= app.right_panel_rect.top()
                                && row < app.right_panel_rect.bottom()
                            {
                                app.input_target = InputTarget::Claude;
                            }
                        }
                        MouseEventKind::Up(MouseButton::Left) => {
                            app.dragging_divider = false;
                        }
                        MouseEventKind::Drag(MouseButton::Left) => {
                            if app.dragging_divider {
                                let total_width =
                                    app.left_panel_rect.width + app.right_panel_rect.width;
                                if total_width > 0 {
                                    let relative_col =
                                        mouse.column.saturating_sub(app.left_panel_rect.x);
                                    let new_pct =
                                        ((relative_col as u32 * 100) / total_width as u32).min(100) as u16;
                                    app.panel_split_percent = new_pct.clamp(20, 80);
                                }
                            }
                        }
                        MouseEventKind::ScrollUp => {
                            // Scroll up = move viewport toward older content = decrease offset
                            if mouse.column < app.divider_col {
                                app.tui_scroll_offset =
                                    app.tui_scroll_offset.saturating_sub(3);
                            } else {
                                app.claude_session.scroll_offset =
                                    app.claude_session.scroll_offset.saturating_sub(3);
                                app.claude_session.auto_scroll = false;
                            }
                        }
                        MouseEventKind::ScrollDown => {
                            // Scroll down = move viewport toward newer content = increase offset
                            if mouse.column < app.divider_col {
                                app.tui_scroll_offset =
                                    app.tui_scroll_offset.saturating_add(3);
                            } else {
                                if app.claude_session.scroll_offset == 0 {
                                    app.claude_session.auto_scroll = true;
                                }
                                app.claude_session.scroll_offset =
                                    app.claude_session.scroll_offset.saturating_add(3);
                            }
                        }
                        _ => {}
                    }
                }
                terminal::AppEvent::Resize(_, _) => {
                    // Terminal handles redraw automatically, just need to not ignore it
                }
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
            // Priority: daemon prompt > local pipeline distilled > raw user message
            let task = app.pending_daemon_prompt.take()
                .or_else(|| app.pending_pipeline_result.as_ref().map(|p| p.distilled.format_for_agent()))
                .unwrap_or_else(|| app.pending_user_message.clone());
            let selected_model = app.pending_pipeline_result.as_ref()
                .map(|p| p.selection.model.clone())
                .unwrap_or_else(|| decision.select_model(&app.config.agents));

            if let Some(ref daemon) = daemon_client {
                let model_clone = selected_model.clone();

                let claude_rx = dispatch::dispatch_to_claude(&task, &agent_name, &selected_model, daemon);
                app.claude_output_rx = Some(claude_rx);
                app.start_claude_session("pending".to_string(), selected_model.clone());
                app.mode = AppMode::Normal;

                app.add_system_message(&format!(
                    "Dispatched to Claude ({}). Watch the right panel.",
                    model_clone
                ));
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
        // Check for explicit cancellation
        let cancel_words = ["no", "n", "cancel", "nevermind", "nah", "stop"];
        if cancel_words.contains(&lower.as_str()) {
            app.mode = AppMode::Normal;
            app.pending_decision = None;
            app.pending_daemon_prompt = None;
            app.pending_pipeline_result = None;
            app.add_system_message("Cancelled.");
            return;
        }

        // Anything else = edit instruction -- refine the prompt
        if let Some(ref mut prompt) = app.pending_daemon_prompt {
            *prompt = format!("{}\n\nAdditional instruction: {}", prompt, msg);
            let updated = format!("--- Updated prompt ---\n{}", prompt);
            app.claude_session.messages.push(updated);
            app.add_system_message("Prompt updated. Say 'yes' to proceed or keep refining.");
            return;
        }
        // If there's a pipeline result, update the distilled objective
        if let Some(ref mut result) = app.pending_pipeline_result {
            result.distilled.objective = format!("{}\n\nAdditional: {}", result.distilled.objective, msg);
            let updated = format!("--- Updated prompt ---\n{}", result.distilled.format_for_display());
            app.claude_session.messages.push(updated);
            app.add_system_message("Prompt updated. Say 'yes' to proceed or keep refining.");
            return;
        }
        // Fallback: treat as cancel
        app.mode = AppMode::Normal;
        app.pending_decision = None;
        app.pending_pipeline_result = None;
        app.pending_daemon_prompt = None;
    }
}
