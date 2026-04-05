use crate::config::Config;
use crate::daemon::client::DaemonClient;
use crate::llm::sidecar::SidecarStatus;
use crate::tui::theme::Theme;
use crate::tui::animation::AnimationState;
use crate::tui::widgets::chat_area::ChatMessage;
use crate::conversation::router::RoutingDecision;
use crate::conversation::manager::ConversationManager;
use crate::intelligence::pipeline::PipelineResult;
use crate::intelligence::synthesizer::SynthesizedResponse;
use crate::dataset::collector::{DatasetCollector, TrainingExample};
use tokio::sync::{mpsc, oneshot};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppMode {
    Normal,
    Routing,
    Generating,
    Optimizing, // Intelligence pipeline running (compress -> distill -> select)
    AwaitingConfirmation,
    AgentActive { session_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceState {
    Connected,
    Disconnected,
    Unavailable,
}

pub struct App {
    pub config: Config,
    pub theme: &'static Theme,
    pub animation: AnimationState,
    pub mode: AppMode,
    pub should_quit: bool,
    pub input: String,
    pub cursor_pos: usize,
    pub conversation: ConversationManager,
    pub scroll_offset: usize,
    pub show_raw_output: bool,
    pub show_agent_panel: bool,
    pub agent_outputs: std::collections::HashMap<String, Vec<String>>,
    pub context_tokens: u32,
    pub token_rx: Option<mpsc::UnboundedReceiver<String>>,
    pub pending_response: String,
    pub sidecar_status: SidecarStatus,
    pub daemon_state: ServiceState,
    pub engram_state: ServiceState,

    // Conductor state
    pub routing_rx: Option<oneshot::Receiver<Result<RoutingDecision, String>>>,
    pub pending_decision: Option<RoutingDecision>,
    pub pending_user_message: String,
    pub stream_abort: Option<tokio::task::AbortHandle>,
    pub collector: DatasetCollector,

    // Intelligence pipeline result (pending approval)
    pub pipeline_rx: Option<oneshot::Receiver<PipelineResult>>,
    pub pending_pipeline_result: Option<PipelineResult>,

    // Last synthesized response (for Ctrl+E raw output toggle)
    pub last_synthesized: Option<SynthesizedResponse>,
    // Pending synthesis result
    pub synthesis_rx: Option<oneshot::Receiver<SynthesizedResponse>>,

    // Channel for async command results (daemon slash commands, etc.)
    pub system_msg_rx: mpsc::UnboundedReceiver<String>,
    // Pending daemon reconnect result
    pub daemon_reconnect_rx: Option<oneshot::Receiver<Result<DaemonClient, String>>>,
}

impl App {
    pub fn new(config: Config, system_prompt: String, system_msg_rx: mpsc::UnboundedReceiver<String>) -> Self {
        let theme_name = config.tui.theme.clone();
        let theme = Theme::by_name(&theme_name).unwrap_or(&crate::tui::theme::THEMES[0]);
        let animation = AnimationState::new(config.tui.animations, config.tui.fps);
        let max_messages = config.session.max_context_messages;
        let max_tokens = config.llm.context_length;

        let dataset_path = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("eidolon")
            .join("training.jsonl");
        let collector = DatasetCollector::new(dataset_path);

        let mut conversation = ConversationManager::new(&system_prompt, max_tokens, max_messages);
        // Add initial greeting
        conversation.add_assistant_message("Yo. The strongest just came online. What do you need?");

        Self {
            config,
            theme,
            animation,
            mode: AppMode::Normal,
            should_quit: false,
            input: String::new(),
            cursor_pos: 0,
            conversation,
            scroll_offset: 0,
            show_raw_output: false,
            show_agent_panel: false,
            agent_outputs: std::collections::HashMap::new(),
            context_tokens: 0,
            token_rx: None,
            pending_response: String::new(),
            sidecar_status: SidecarStatus::Stopped,
            daemon_state: ServiceState::Disconnected,
            engram_state: ServiceState::Disconnected,
            routing_rx: None,
            pending_decision: None,
            pending_user_message: String::new(),
            stream_abort: None,
            collector,
            pipeline_rx: None,
            pending_pipeline_result: None,
            last_synthesized: None,
            synthesis_rx: None,
            system_msg_rx,
            daemon_reconnect_rx: None,
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
        self.conversation.add_user_message(&msg);
        self.input.clear();
        self.cursor_pos = 0;
        Some(msg)
    }

    pub fn add_system_message(&mut self, content: &str) {
        self.conversation.add_assistant_message(content);
    }

    pub fn clear_messages(&mut self) {
        self.conversation.clear();
    }

    pub fn display_messages(&self) -> Vec<ChatMessage> {
        self.conversation.display_messages()
    }

    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(3);
    }

    pub fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(3);
    }

    pub fn scroll_page_up(&mut self, page_height: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(page_height);
    }

    pub fn scroll_page_down(&mut self, page_height: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(page_height);
    }

    pub fn llm_status(&self) -> SidecarStatus {
        self.sidecar_status.clone()
    }

    pub fn start_streaming(&mut self, rx: mpsc::UnboundedReceiver<String>) {
        self.pending_response.clear();
        self.token_rx = Some(rx);
        self.mode = AppMode::Generating;
    }

    pub fn commit_pending_response(&mut self) {
        if !self.pending_response.is_empty() {
            let response = self.pending_response.clone();

            // Record to dataset
            if !self.pending_user_message.is_empty() {
                let intent = match &self.pending_decision {
                    Some(d) => format!("{:?}", d.intent).to_lowercase(),
                    None => "casual".to_string(),
                };
                let tools_called = self.pending_decision.as_ref()
                    .map(|d| d.tools_needed.clone())
                    .unwrap_or_default();

                let (compression_ratio, model_selected, estimated_cost, pipeline_ran) =
                    if let Some(ref pr) = self.pending_pipeline_result {
                        (
                            pr.compression.as_ref().map(|c| c.compression_ratio),
                            Some(format!("{}/{}", pr.selection.agent, pr.selection.model)),
                            Some(pr.selection.estimated_cost),
                            true,
                        )
                    } else {
                        (None, None, None, false)
                    };

                let example = TrainingExample {
                    system_prompt: self.conversation.system_prompt().to_string(),
                    user_message: self.pending_user_message.clone(),
                    assistant_response: response.clone(),
                    intent,
                    tools_called,
                    user_override: false,
                    compression_ratio,
                    model_selected,
                    user_override_model: None,
                    estimated_cost,
                    pipeline_ran,
                };
                let _ = self.collector.record(example);
                let _ = self.collector.flush();
            }

            self.add_system_message(&response);
            self.pending_response.clear();
            self.pending_decision = None;
            self.pending_user_message.clear();
        }
        self.token_rx = None;
        self.mode = AppMode::Normal;
    }
}
