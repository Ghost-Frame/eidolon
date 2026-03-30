use eidolon_tui::config::Config;
use eidolon_tui::llm::sidecar::SidecarStatus;
use eidolon_tui::tui::theme::Theme;
use eidolon_tui::tui::animation::AnimationState;
use eidolon_tui::tui::widgets::chat_area::ChatMessage;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppMode {
    Normal,
    Generating,
    AgentActive { session_id: String },
}

pub struct App {
    pub config: Config,
    pub theme: &'static Theme,
    pub animation: AnimationState,
    pub mode: AppMode,
    pub should_quit: bool,
    pub input: String,
    pub cursor_pos: usize,
    pub chat_messages: Vec<ChatMessage>,
    pub scroll_offset: u16,
    pub agent_outputs: std::collections::HashMap<String, Vec<String>>,
    pub context_tokens: u32,
    pub token_rx: Option<tokio::sync::mpsc::UnboundedReceiver<String>>,
    pub pending_response: String,
    pub sidecar_status: SidecarStatus,
}

impl App {
    pub fn new(config: Config) -> Self {
        let theme_name = config.tui.theme.clone();
        let theme = Theme::by_name(&theme_name).unwrap_or(&eidolon_tui::tui::theme::THEMES[0]);

        let animation = AnimationState::new(config.tui.animations, config.tui.fps);

        Self {
            config,
            theme,
            animation,
            mode: AppMode::Normal,
            should_quit: false,
            input: String::new(),
            cursor_pos: 0,
            chat_messages: vec![ChatMessage {
                sender: "Gojo".to_string(),
                content: "Yo. The strongest just came online. What do you need?".to_string(),
                is_user: false,
            }],
            scroll_offset: 0,
            agent_outputs: std::collections::HashMap::new(),
            context_tokens: 0,
            token_rx: None,
            pending_response: String::new(),
            sidecar_status: SidecarStatus::Stopped,
        }
    }

    pub fn cycle_theme(&mut self) {
        let next_name = Theme::cycle_next(self.theme.name);
        if let Some(next) = Theme::by_name(next_name) {
            self.theme = next;
        }
    }

    pub fn handle_input_char(&mut self, c: char) {
        let byte_pos = self.input.char_indices()
            .nth(self.cursor_pos)
            .map(|(i, _)| i)
            .unwrap_or(self.input.len());
        self.input.insert(byte_pos, c);
        self.cursor_pos += 1;
    }

    pub fn handle_backspace(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            let byte_pos = self.input.char_indices()
                .nth(self.cursor_pos)
                .map(|(i, _)| i)
                .unwrap_or(self.input.len());
            self.input.remove(byte_pos);
        }
    }

    pub fn submit_input(&mut self) -> Option<String> {
        if self.input.is_empty() {
            return None;
        }
        let msg = self.input.clone();
        self.chat_messages.push(ChatMessage {
            sender: "You".to_string(),
            content: msg.clone(),
            is_user: true,
        });
        self.input.clear();
        self.cursor_pos = 0;
        Some(msg)
    }

    pub fn add_gojo_message(&mut self, content: &str) {
        self.chat_messages.push(ChatMessage {
            sender: "Gojo".to_string(),
            content: content.to_string(),
            is_user: false,
        });
    }

    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(3);
    }

    pub fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(3);
    }

    pub fn llm_status(&self) -> SidecarStatus {
        self.sidecar_status.clone()
    }

    pub fn start_streaming(&mut self, rx: tokio::sync::mpsc::UnboundedReceiver<String>) {
        self.pending_response.clear();
        self.token_rx = Some(rx);
        self.mode = AppMode::Generating;
    }

    pub fn commit_pending_response(&mut self) {
        if !self.pending_response.is_empty() {
            self.add_gojo_message(&self.pending_response.clone());
            self.pending_response.clear();
        }
        self.token_rx = None;
        self.mode = AppMode::Normal;
    }
}
