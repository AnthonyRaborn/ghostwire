//! Effects applied to the rendered buffer after widgets draw.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::theme;

/// Signal decay: pull every foreground color in `area` toward the dim background tone.
pub fn fade(buf: &mut Buffer, area: Rect, amount: f32) {
    let area = area.intersection(buf.area);
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.fg = theme::mix(cell.fg, theme::DIM, amount);
            }
        }
    }
}
