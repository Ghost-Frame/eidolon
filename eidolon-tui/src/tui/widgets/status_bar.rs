use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};
use crate::tui::theme::Theme;
use crate::tui::animation::AnimationState;
use crate::llm::sidecar::SidecarStatus;
use crate::app::{InputTarget, ClaudeSessionState};

pub struct StatusBar<'a> {
    theme: &'a Theme,
    animation: &'a AnimationState,
    llm_status: &'a SidecarStatus,
    context_tokens: u32,
    max_context_tokens: u32,
    active_agents: u32,
    input_target: InputTarget,
    claude_state: &'a ClaudeSessionState,
}

impl<'a> StatusBar<'a> {
    pub fn new(
        theme: &'a Theme,
        animation: &'a AnimationState,
        llm_status: &'a SidecarStatus,
        context_tokens: u32,
        max_context_tokens: u32,
        active_agents: u32,
        input_target: InputTarget,
        claude_state: &'a ClaudeSessionState,
    ) -> Self {
        Self { theme, animation, llm_status, context_tokens, max_context_tokens, active_agents, input_target, claude_state }
    }
}

impl Widget for StatusBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Background
        for x in area.left()..area.right() {
            buf[(x, area.y)].set_style(Style::default().bg(self.theme.bg_secondary));
        }

        // Infinity symbol with pulse animation
        let pulse = self.animation.pulse(2.0);
        let (pr, pg, pb) = color_to_rgb(self.theme.pulse_start);
        let (er, eg, eb) = color_to_rgb(self.theme.pulse_end);
        let (r, g, b) = AnimationState::lerp_color((pr, pg, pb), (er, eg, eb), pulse);
        let infinity_color = Color::Rgb(r, g, b);

        let mut x = area.left() + 1;

        // "eidolon v2"
        let title = "eidolon v2";
        for ch in title.chars() {
            if x < area.right() {
                buf[(x, area.y)].set_char(ch).set_style(Style::default().fg(self.theme.accent));
                x += 1;
            }
        }
        x += 1;

        // Infinity symbol
        let infinity = "\u{221E}";
        for ch in infinity.chars() {
            if x < area.right() {
                buf[(x, area.y)].set_char(ch).set_style(Style::default().fg(infinity_color));
                x += 1;
            }
        }
        x += 2;

        // "eidolon mode"
        let mode = "eidolon";
        for ch in mode.chars() {
            if x < area.right() {
                buf[(x, area.y)].set_char(ch).set_style(Style::default().fg(self.theme.dim));
                x += 1;
            }
        }
        x += 2;

        // Input target indicator
        let (target_str, target_color) = match self.input_target {
            InputTarget::Tui => ("[TUI]", self.theme.eidolon_text),
            InputTarget::Claude => ("[Claude]", self.theme.accent),
        };
        for ch in target_str.chars() {
            if x < area.right() {
                buf[(x, area.y)].set_char(ch).set_style(Style::default().fg(target_color));
                x += 1;
            }
        }
        x += 1;

        // Claude session state
        let (claude_str, claude_color) = match self.claude_state {
            ClaudeSessionState::Idle => ("claude:idle", self.theme.dim),
            ClaudeSessionState::Starting => ("claude:starting", self.theme.warning),
            ClaudeSessionState::Active { .. } => ("claude:active", self.theme.success),
            ClaudeSessionState::Completed { .. } => ("claude:done", self.theme.text_secondary),
            ClaudeSessionState::Failed { .. } => ("claude:err", self.theme.error),
        };
        for ch in claude_str.chars() {
            if x < area.right() {
                buf[(x, area.y)].set_char(ch).set_style(Style::default().fg(claude_color));
                x += 1;
            }
        }

        // Right-aligned: LLM status, context usage, theme
        let llm_str = match self.llm_status {
            SidecarStatus::Ready => "llm: ready",
            SidecarStatus::Starting => "llm: starting...",
            SidecarStatus::Stopped => "llm: stopped",
            SidecarStatus::Degraded(_) => "llm: degraded",
            SidecarStatus::Error(_) => "llm: error",
        };
        let llm_color = match self.llm_status {
            SidecarStatus::Ready => self.theme.success,
            SidecarStatus::Starting => self.theme.warning,
            SidecarStatus::Degraded(_) => self.theme.warning,
            _ => self.theme.error,
        };

        let ctx_str = format!("ctx: {:.1}k/{:.0}k", self.context_tokens as f64 / 1000.0, self.max_context_tokens as f64 / 1000.0);
        let agents_str = if self.active_agents > 0 {
            format!("agents: {}", self.active_agents)
        } else {
            String::new()
        };
        let theme_str = format!("theme: {}", self.theme.name);

        let right_parts = format!("[{}] [{}] {} [{}]", llm_str, ctx_str, agents_str, theme_str);
        let right_start = area.right().saturating_sub(right_parts.len() as u16 + 1);

        // Track character position to determine color; avoid byte-slicing which panics on
        // multibyte characters. The "llm:" segment is always the first bracketed item in
        // right_parts, so we just need to know whether we've passed the closing ']' of that
        // segment. We detect this by character count rather than byte offset.
        let llm_segment_end = right_parts.find(']').map(|b| {
            // Convert byte offset to char index
            right_parts[..=b].chars().count()
        }).unwrap_or(0);

        let mut rx = right_start;
        for (char_idx, ch) in right_parts.chars().enumerate() {
            if rx < area.right() {
                let color = if char_idx < llm_segment_end {
                    llm_color
                } else {
                    self.theme.dim
                };
                buf[(rx, area.y)].set_char(ch).set_style(Style::default().fg(color));
                rx += 1;
            }
        }
    }
}

fn color_to_rgb(color: Color) -> (u8, u8, u8) {
    match color {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (128, 128, 128),
    }
}
