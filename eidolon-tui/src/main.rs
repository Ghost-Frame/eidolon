use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

mod app;

use eidolon_tui::config::Config;
use eidolon_tui::llm::client::LlmClient;
use eidolon_tui::llm::sidecar::{LlamaSidecar, SidecarStatus};
use eidolon_tui::conversation::personality::gojo_system_prompt;
use eidolon_tui::tui::terminal;
use eidolon_tui::tui::widgets::{
    status_bar::StatusBar,
    chat_area::ChatArea,
    input_bar::InputBar,
    thinking::ThinkingSpinner,
};

use app::{App, AppMode};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load().unwrap_or_default();

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = terminal::restore();
        default_hook(info);
    }));

    let mut tui = terminal::init()?;
    let mut app = App::new(config);

    // Determine LLM base URL
    let llm_base_url = app.config.llm.base_url.clone()
        .unwrap_or_else(|| format!("http://localhost:{}", app.config.llm.port));

    let llm_client = Arc::new(LlmClient::new(&llm_base_url));

    // Spawn sidecar only if model_path is set and no remote base_url configured
    let _sidecar_handle = if app.config.llm.base_url.is_none() && !app.config.llm.model_path.is_empty() {
        let model_path = app.config.llm.model_path.clone();
        let port = app.config.llm.port;
        let ctx = app.config.llm.context_length;
        let ngl = app.config.llm.gpu_layers;
        Some(tokio::spawn(async move {
            let mut sidecar = LlamaSidecar::new(&model_path, port, ctx, ngl);
            let _ = sidecar.start().await;
            sidecar
        }))
    } else {
        // If base_url is set or no model, mark as Ready (assume external server)
        if app.config.llm.base_url.is_some() {
            app.sidecar_status = SidecarStatus::Ready;
        }
        None
    };

    let system_prompt = gojo_system_prompt();

    loop {
        if app.should_quit {
            break;
        }

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
            if app.mode == AppMode::Generating && !app.pending_response.is_empty() {
                display_messages.push(eidolon_tui::tui::widgets::chat_area::ChatMessage {
                    sender: "Gojo".to_string(),
                    content: app.pending_response.clone(),
                    is_user: false,
                });
            }
            let chat = ChatArea::new(app.theme, &display_messages, app.scroll_offset);
            frame.render_widget(chat, chunks[1]);

            if app.mode == AppMode::Generating {
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
                                    // Slash command handling
                                    if msg.starts_with("/theme ") {
                                        let name = msg.trim_start_matches("/theme ").trim();
                                        if eidolon_tui::tui::theme::Theme::by_name(name).is_some() {
                                            app.cycle_theme();
                                            app.add_gojo_message("Theme switched. Looking good.");
                                        } else {
                                            app.add_gojo_message(&format!("No theme called '{}'. Nice try.", name));
                                        }
                                    } else if msg == "/quit" || msg == "/exit" {
                                        app.should_quit = true;
                                    } else if msg == "/status" {
                                        app.add_gojo_message("Everything's running. Relax.");
                                    } else {
                                        app.add_gojo_message(&format!("Unknown command: {}", msg));
                                    }
                                } else if app.mode == AppMode::Normal {
                                    // Build conversation history from chat messages
                                    let history: Vec<(String, String)> = app.chat_messages.iter()
                                        .map(|m| {
                                            let role = if m.is_user { "user".to_string() } else { "assistant".to_string() };
                                            (role, m.content.clone())
                                        })
                                        .collect();

                                    let (tx, rx) = mpsc::unbounded_channel::<String>();
                                    app.start_streaming(rx);

                                    let client = llm_client.clone();
                                    let sys = system_prompt.clone();
                                    let temperature = app.config.llm.temperature_casual;

                                    tokio::spawn(async move {
                                        // Build messages: system + history
                                        let mut msgs: Vec<(&str, String)> = vec![("system", sys)];
                                        for (role, content) in &history {
                                            msgs.push((role.as_str(), content.clone()));
                                        }

                                        let msg_refs: Vec<(&str, &str)> = msgs.iter()
                                            .map(|(r, c)| (*r, c.as_str()))
                                            .collect();

                                        let request = LlmClient::build_request(&msg_refs, temperature, None);
                                        let _ = client.stream_complete(&request, tx).await;
                                    });
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

        // Drain incoming LLM tokens
        if app.mode == AppMode::Generating {
            let mut done = false;
            if let Some(rx) = app.token_rx.as_mut() {
                loop {
                    match rx.try_recv() {
                        Ok(token) => {
                            app.pending_response.push_str(&token);
                        }
                        Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                        Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
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
    }

    terminal::restore()?;
    Ok(())
}
