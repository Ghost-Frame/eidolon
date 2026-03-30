use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::Widget,
};
use crate::tui::theme::Theme;
use crate::tui::animation::AnimationState;

const BRAILLE_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

pub struct ThinkingSpinner<'a> {
    theme: &'a Theme,
    animation: &'a AnimationState,
}

impl<'a> ThinkingSpinner<'a> {
    pub fn new(theme: &'a Theme, animation: &'a AnimationState) -> Self {
        Self { theme, animation }
    }
}

impl Widget for ThinkingSpinner<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 12 || area.height < 1 {
            return;
        }

        let frame_idx = ((self.animation.elapsed_secs() * 10.0) as usize) % BRAILLE_FRAMES.len();
        let spinner = BRAILLE_FRAMES[frame_idx];

        let text = format!("{} thinking...", spinner);
        let x = area.left();
        let y = area.top();

        for (i, ch) in text.chars().enumerate() {
            let xi = x + i as u16;
            if xi < area.right() {
                buf[(xi, y)].set_char(ch).set_style(
                    Style::default().fg(self.theme.thinking)
                );
            }
        }
    }
}
