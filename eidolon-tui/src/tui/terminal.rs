// src/tui/terminal.rs
use crossterm::{
    event::{self, Event, KeyEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{self, Stdout};
use std::time::Duration;

pub type Tui = Terminal<CrosstermBackend<Stdout>>;

/// Initialize the terminal for TUI rendering.
pub fn init() -> io::Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

/// Restore the terminal to its original state.
pub fn restore() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

/// TUI event types.
pub enum AppEvent {
    Key(KeyEvent),
    Tick,
    LlmToken(String),
    AgentOutput { session_id: String, line: String },
    LlmDone(String),
    Error(String),
}

/// Check for keyboard input with timeout.
pub fn poll_event(timeout: Duration) -> Option<AppEvent> {
    if event::poll(timeout).ok()? {
        if let Ok(Event::Key(key)) = event::read() {
            return Some(AppEvent::Key(key));
        }
    }
    Some(AppEvent::Tick)
}
