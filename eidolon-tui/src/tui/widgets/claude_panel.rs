use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
              StatefulWidget, Widget, Wrap},
};
use crate::tui::theme::Theme;
use crate::tui::animation::AnimationState;
use crate::app::{ClaudeSession, ClaudeSessionState};

pub struct ClaudePanel<'a> {
    theme: &'a Theme,
    animation: &'a AnimationState,
    session: &'a ClaudeSession,
    is_focused: bool,
    elapsed_secs: u64,
}

impl<'a> ClaudePanel<'a> {
    pub fn new(
        theme: &'a Theme,
        animation: &'a AnimationState,
        session: &'a ClaudeSession,
        is_focused: bool,
        elapsed_secs: u64,
    ) -> Self {
        Self { theme, animation, session, is_focused, elapsed_secs }
    }
}

impl Widget for ClaudePanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Border color: pulsing accent when focused, dim when not
        let border_color = if self.is_focused {
            let pulse = self.animation.pulse(4.0);
            let (sr, sg, sb) = color_to_rgb(self.theme.claude_border);
            let (dr, dg, db) = color_to_rgb(self.theme.dim);
            let (r, g, b) = AnimationState::lerp_color((sr, sg, sb), (dr, dg, db), pulse * 0.3);
            Color::Rgb(r, g, b)
        } else {
            self.theme.dim
        };

        let title = match &self.session.state {
            ClaudeSessionState::Idle => " Claude -- idle ".to_string(),
            ClaudeSessionState::Starting => " Claude -- starting... ".to_string(),
            ClaudeSessionState::Active { .. } => {
                let model = if self.session.model.is_empty() {
                    "claude".to_string()
                } else {
                    self.session.model.clone()
                };
                format!(" Claude | {} | {}s ", model, self.elapsed_secs)
            }
            ClaudeSessionState::Completed { exit_code, .. } => {
                format!(" Claude -- done (exit {}) ", exit_code)
            }
            ClaudeSessionState::Failed { .. } => " Claude -- error ".to_string(),
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(title, Style::default().fg(self.theme.claude_text)))
            .style(Style::default().bg(self.theme.bg));

        let inner = block.inner(area);
        block.render(area, buf);

        match &self.session.state {
            ClaudeSessionState::Idle => {
                let text = Line::from(Span::styled(
                    "No active session.",
                    Style::default().fg(self.theme.dim),
                ));
                Paragraph::new(vec![text])
                    .render(inner, buf);
            }
            _ => {
                let max_lines = inner.height as usize;
                let total = self.session.messages.len();
                let start = if self.session.auto_scroll {
                    total.saturating_sub(max_lines)
                } else {
                    // scroll_offset counts from bottom: 0 = bottom
                    let bottom_start = total.saturating_sub(max_lines);
                    bottom_start.saturating_sub(self.session.scroll_offset)
                };
                let end = (start + max_lines).min(total);

                let visible: Vec<Line> = self.session.messages[start..end]
                    .iter()
                    .map(|l| {
                        let color = line_color(l, self.theme);
                        Line::from(Span::styled(l.as_str(), Style::default().fg(color)))
                    })
                    .collect();

                let show_scrollbar = total > max_lines;

                let para_area = if show_scrollbar && inner.width > 1 {
                    Rect {
                        width: inner.width - 1,
                        ..inner
                    }
                } else {
                    inner
                };

                Paragraph::new(visible)
                    .wrap(Wrap { trim: false })
                    .render(para_area, buf);

                if show_scrollbar {
                    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                        .begin_symbol(None)
                        .end_symbol(None)
                        .thumb_style(Style::default().fg(self.theme.claude_border))
                        .track_style(Style::default().fg(self.theme.dim));

                    let mut scrollbar_state = ScrollbarState::new(total)
                        .position(start)
                        .viewport_content_length(max_lines);

                    let scrollbar_area = Rect {
                        x: inner.x + inner.width - 1,
                        y: inner.y,
                        width: 1,
                        height: inner.height,
                    };

                    scrollbar.render(scrollbar_area, buf, &mut scrollbar_state);
                }
            }
        }
    }
}

fn line_color(line: &str, theme: &Theme) -> Color {
    if line.starts_with("> You:") {
        theme.user_text
    } else if line.contains("[error]") {
        Color::Rgb(239, 68, 68)
    } else if line.contains("[warning]") {
        Color::Rgb(250, 204, 21)
    } else if line.starts_with('>') {
        theme.accent
    } else {
        theme.text_secondary
    }
}

fn color_to_rgb(color: Color) -> (u8, u8, u8) {
    match color {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (128, 128, 128),
    }
}
