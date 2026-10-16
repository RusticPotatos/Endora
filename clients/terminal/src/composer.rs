use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Borders, Paragraph},
};
use unicode_width::UnicodeWidthStr;

/// Keep the insertion point visible without altering the stored draft.
pub fn window(input: &str, cursor: usize, width: u16) -> (&str, u16) {
    let mut start = 0;
    let budget = usize::from(width.saturating_sub(1));
    for (offset, ch) in input[..cursor].char_indices() {
        if input[start..cursor].width() <= budget {
            break;
        }
        start = offset + ch.len_utf8();
    }
    (
        &input[start..],
        input[start..cursor].width().min(budget) as u16,
    )
}

pub fn draw(frame: &mut Frame<'_>, area: Rect, input: &str, cursor: usize) {
    let block = Block::default().title("Message").borders(Borders::ALL);
    let inner = block.inner(area);
    let (visible, x) = window(input, cursor, inner.width);
    frame.render_widget(Paragraph::new(visible).block(block), area);
    if inner.width > 0 && inner.height > 0 {
        frame.set_cursor_position((inner.x + x, inner.y));
    }
}
