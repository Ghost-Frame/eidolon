use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
              StatefulWidget, Widget, Wrap},
};
use crate::tui::theme::Theme;

#[derive(Clone)]
pub struct ChatMessage {
    pub sender: String,
    pub content: String,
    pub is_user: bool,
}

pub struct ChatArea<'a> {
    theme: &'a Theme,
    messages: &'a [ChatMessage],
    scroll_offset: usize,
    is_focused: bool,
}

impl<'a> ChatArea<'a> {
    pub fn new(theme: &'a Theme, messages: &'a [ChatMessage], scroll_offset: usize, is_focused: bool) -> Self {
        Self { theme, messages, scroll_offset, is_focused }
    }
}

impl Widget for ChatArea<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let border_color = if self.is_focused {
            self.theme.accent
        } else {
            self.theme.dim
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(" TUI ", Style::default().fg(self.theme.accent)))
            .style(Style::default().bg(self.theme.bg));

        let inner = block.inner(area);
        block.render(area, buf);

        let mut lines: Vec<Line> = Vec::new();
        for msg in self.messages {
            let color = if msg.is_user {
                self.theme.user_text
            } else {
                self.theme.gojo_text
            };

            lines.push(Line::from(vec![
                Span::styled(
                    format!("{}: ", msg.sender),
                    Style::default().fg(color),
                ),
            ]));

            for line in msg.content.lines() {
                lines.push(Line::from(Span::styled(
                    format!("  {}", line),
                    Style::default().fg(self.theme.text),
                )));
            }
            lines.push(Line::default());
        }

        let content_length = lines.len();
        let viewport_height = inner.height as usize;
        let show_scrollbar = content_length > viewport_height;

        // Shrink the paragraph area by 1 column on the right when showing a scrollbar
        let para_area = if show_scrollbar && inner.width > 1 {
            Rect {
                width: inner.width - 1,
                ..inner
            }
        } else {
            inner
        };

        let paragraph = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset as u16, 0));

        paragraph.render(para_area, buf);

        if show_scrollbar {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .thumb_style(Style::default().fg(self.theme.accent))
                .track_style(Style::default().fg(self.theme.dim));

            let mut scrollbar_state = ScrollbarState::new(content_length)
                .position(self.scroll_offset)
                .viewport_content_length(viewport_height);

            // Render scrollbar in the rightmost column of inner, within the border
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
