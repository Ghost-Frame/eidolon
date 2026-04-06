// src/tui/terminal.rs
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEvent, KeyEventKind, MouseEvent},
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
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

/// Restore the terminal to its original state.
pub fn restore() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;
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
    Mouse(MouseEvent),
    Resize(u16, u16),
}

/// Check for keyboard/mouse input with timeout.
pub fn poll_event(timeout: Duration, mouse_enabled: bool) -> Option<AppEvent> {
    if event::poll(timeout).ok()? {
        match event::read() {
            Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                return Some(AppEvent::Key(key));
            }
            Ok(Event::Mouse(mouse)) if mouse_enabled => {
                return Some(AppEvent::Mouse(mouse));
            }
            Ok(Event::Resize(cols, rows)) => {
                return Some(AppEvent::Resize(cols, rows));
            }
            Err(_) => return None, // I/O error -- propagate as no event
            _ => {} // Non-press key, disabled mouse, etc -- fall through to Tick
        }
    }
    Some(AppEvent::Tick)
}
