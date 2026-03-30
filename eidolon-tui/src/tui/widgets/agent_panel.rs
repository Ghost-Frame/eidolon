use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};
use crate::tui::theme::Theme;
use crate::tui::animation::AnimationState;

pub struct AgentPanel<'a> {
    theme: &'a Theme,
    animation: &'a AnimationState,
    agent_type: &'a str,
    task: &'a str,
    elapsed_secs: u64,
    output_lines: &'a [String],
    is_active: bool,
}

impl<'a> AgentPanel<'a> {
    pub fn new(
        theme: &'a Theme,
        animation: &'a AnimationState,
        agent_type: &'a str,
        task: &'a str,
        elapsed_secs: u64,
        output_lines: &'a [String],
        is_active: bool,
    ) -> Self {
        Self { theme, animation, agent_type, task, elapsed_secs, output_lines, is_active }
    }
}

impl Widget for AgentPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let border_color = if self.is_active {
            let pulse = self.animation.pulse(4.0);
            let (sr, sg, sb) = color_to_rgb(self.theme.agent_border);
            let (dr, dg, db) = color_to_rgb(self.theme.dim);
            let (r, g, b) = AnimationState::lerp_color((sr, sg, sb), (dr, dg, db), pulse * 0.3);
            Color::Rgb(r, g, b)
        } else {
            self.theme.dim
        };

        let title = format!(
            " {} | {} | {}s ",
            self.agent_type, self.task,
            self.elapsed_secs,
        );

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(title, Style::default().fg(self.theme.accent)))
            .style(Style::default().bg(self.theme.bg_secondary));

        let inner = block.inner(area);
        block.render(area, buf);

        let max_lines = inner.height as usize;
        let start = self.output_lines.len().saturating_sub(max_lines);
        let visible: Vec<Line> = self.output_lines[start..]
            .iter()
            .map(|l| Line::from(Span::styled(l.as_str(), Style::default().fg(self.theme.text_secondary))))
            .collect();

        Paragraph::new(visible)
            .wrap(Wrap { trim: false })
            .render(inner, buf);
    }
}

fn color_to_rgb(color: Color) -> (u8, u8, u8) {
    match color {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (128, 128, 128),
    }
}
