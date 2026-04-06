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
                let viewport_height = inner.height as usize;
                // Reserve 1 column for scrollbar when needed
                let content_width = if inner.width > 1 { inner.width - 1 } else { inner.width };
                let cw = content_width.max(1) as usize;

                // Build all lines with styling
                let all_lines: Vec<Line> = self.session.messages.iter()
                    .map(|l| {
                        let color = line_color(l, self.theme);
                        Line::from(Span::styled(l.as_str(), Style::default().fg(color)))
                    })
                    .collect();

                // Estimate total wrapped row count for scroll math.
                // Slightly overestimate to account for word-wrap padding --
                // ratatui breaks on word boundaries, not character positions,
                // so actual rows can exceed simple ceil(len/width).
                let total_rows: usize = self.session.messages.iter()
                    .map(|l| {
                        let byte_len = l.len();
                        if byte_len <= cw { 1 } else { byte_len / cw + 1 }
                    })
                    .sum();

                let show_scrollbar = total_rows > viewport_height;

                // Calculate scroll position (rows from top)
                let scroll_pos = if self.session.auto_scroll {
                    total_rows.saturating_sub(viewport_height)
                } else {
                    let bottom = total_rows.saturating_sub(viewport_height);
                    bottom.saturating_sub(self.session.scroll_offset)
                };

                let para_area = if show_scrollbar && inner.width > 1 {
                    Rect { width: inner.width - 1, ..inner }
                } else {
                    inner
                };

                Paragraph::new(all_lines)
                    .wrap(Wrap { trim: false })
                    .scroll((scroll_pos as u16, 0))
                    .render(para_area, buf);

                if show_scrollbar {
                    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                        .begin_symbol(None)
                        .end_symbol(None)
                        .thumb_style(Style::default().fg(self.theme.claude_border))
                        .track_style(Style::default().fg(self.theme.dim));

                    let mut scrollbar_state = ScrollbarState::new(total_rows)
                        .position(scroll_pos)
                        .viewport_content_length(viewport_height);

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
    } else if line.contains("PERMISSION REQUIRED") {
        Color::Rgb(250, 204, 21)
    } else if line.starts_with("  Type 'y'") {
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
