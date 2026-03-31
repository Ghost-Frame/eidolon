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
        let prompt_len = prompt.len();
        let visible_width = (inner.width as usize).saturating_sub(prompt_len);

        // Calculate scroll offset so cursor is always visible
        let cursor_pos = self.cursor_pos.min(self.content.len());
        let scroll_offset = if cursor_pos >= visible_width {
            cursor_pos - visible_width + 1
        } else {
            0
        };

        // Get the visible slice of content (char-aware)
        let chars: Vec<char> = self.content.chars().collect();
        let visible_start = scroll_offset;
        let visible_end = (scroll_offset + visible_width).min(chars.len());
        let cursor_in_view = cursor_pos - scroll_offset;

        let visible_before: String = chars[visible_start..cursor_pos.min(visible_end)].iter().collect();
        let cursor_char = if cursor_pos < chars.len() {
            chars[cursor_pos].to_string()
        } else {
            " ".to_string()
        };
        let after_pos = (cursor_pos + 1).min(chars.len());
        let visible_after: String = chars[after_pos..visible_end.max(after_pos)].iter().collect();

        let cursor_style = Style::default()
            .fg(self.theme.bg)
            .bg(self.theme.accent);

        // Show scroll indicator when content is clipped on the left
        let display_prompt = if scroll_offset > 0 { "<" } else { prompt };

        let paragraph = Paragraph::new(Line::from(vec![
            Span::styled(display_prompt, Style::default().fg(self.theme.accent)),
            Span::styled(visible_before, Style::default().fg(self.theme.user_text)),
            Span::styled(cursor_char, cursor_style),
            Span::styled(visible_after, Style::default().fg(self.theme.user_text)),
        ]));

        paragraph.render(inner, buf);
    }
}
