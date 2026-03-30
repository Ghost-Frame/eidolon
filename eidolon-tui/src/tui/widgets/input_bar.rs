use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};
use crate::tui::theme::Theme;

pub struct InputBar<'a> {
    theme: &'a Theme,
    content: &'a str,
    cursor_pos: usize,
}

impl<'a> InputBar<'a> {
    pub fn new(theme: &'a Theme, content: &'a str, cursor_pos: usize) -> Self {
        Self { theme, content, cursor_pos }
    }
}

impl Widget for InputBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(self.theme.dim))
            .style(Style::default().bg(self.theme.bg));

        let inner = block.inner(area);
        block.render(area, buf);

        let prompt = "> ";

        let paragraph = Paragraph::new(Line::from(vec![
            Span::styled(prompt, Style::default().fg(self.theme.accent)),
            Span::styled(self.content, Style::default().fg(self.theme.user_text)),
        ]));

        paragraph.render(inner, buf);
    }
}
