use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};
use crate::tui::theme::Theme;
use crate::tui::animation::AnimationState;
use crate::llm::sidecar::SidecarStatus;

pub struct StatusBar<'a> {
    theme: &'a Theme,
    animation: &'a AnimationState,
    llm_status: &'a SidecarStatus,
    context_tokens: u32,
    max_context_tokens: u32,
    active_agents: u32,
}

impl<'a> StatusBar<'a> {
    pub fn new(
        theme: &'a Theme,
        animation: &'a AnimationState,
        llm_status: &'a SidecarStatus,
        context_tokens: u32,
        max_context_tokens: u32,
        active_agents: u32,
    ) -> Self {
        Self { theme, animation, llm_status, context_tokens, max_context_tokens, active_agents }
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

        // "gojo mode"
        let mode = "gojo mode";
        for ch in mode.chars() {
            if x < area.right() {
                buf[(x, area.y)].set_char(ch).set_style(Style::default().fg(self.theme.dim));
                x += 1;
            }
        }

        // Right-aligned: LLM status, context usage, theme
        let llm_str = match self.llm_status {
            SidecarStatus::Ready => "llm: ready",
            SidecarStatus::Starting => "llm: starting...",
            SidecarStatus::Stopped => "llm: stopped",
            SidecarStatus::Error(_) => "llm: error",
        };
        let llm_color = match self.llm_status {
            SidecarStatus::Ready => self.theme.success,
            SidecarStatus::Starting => self.theme.warning,
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

        let mut rx = right_start;
        for ch in right_parts.chars() {
            if rx < area.right() {
                let color = if right_parts[..(rx - right_start) as usize + 1].contains("llm:") {
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
