use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};
use crate::tui::theme::Theme;

pub struct ChatMessage {
    pub sender: String,
    pub content: String,
    pub is_user: bool,
}

pub struct ChatArea<'a> {
    theme: &'a Theme,
    messages: &'a [ChatMessage],
    scroll_offset: u16,
}

impl<'a> ChatArea<'a> {
    pub fn new(theme: &'a Theme, messages: &'a [ChatMessage], scroll_offset: u16) -> Self {
        Self { theme, messages, scroll_offset }
    }
}

impl Widget for ChatArea<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::NONE)
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

        let paragraph = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset, 0));

        paragraph.render(inner, buf);
    }
}
