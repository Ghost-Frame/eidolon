use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout};
use std::time::Duration;

mod app;

use eidolon_tui::config::Config;
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

    let mut tui = terminal::init()?;
    let mut app = App::new(config);

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

            let chat = ChatArea::new(app.theme, &app.chat_messages, app.scroll_offset);
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
                                if msg.starts_with("/theme ") {
                                    let name = msg.trim_start_matches("/theme ").trim();
                                    if eidolon_tui::tui::theme::Theme::by_name(name).is_some() {
                                        app.cycle_theme();
                                        app.add_gojo_message(&format!("Theme switched. Looking good."));
                                    } else {
                                        app.add_gojo_message(&format!("No theme called '{}'. Nice try.", name));
                                    }
                                } else if msg == "/quit" || msg == "/exit" {
                                    app.should_quit = true;
                                } else if msg == "/status" {
                                    app.add_gojo_message("Everything's running. Relax.");
                                } else {
                                    app.add_gojo_message("I hear you. LLM integration coming soon -- right now I'm just the pretty face.");
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
