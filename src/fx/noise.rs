//! Low-level texture: decay noise on stale data, chaotic-mode sparkle, and scanlines.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::{hash, unit};
use crate::theme;

const DECAY_GLYPHS: &[char] = &['░', '▒', '·', ':'];
const HEX: &[u8] = b"0123456789abcdef";
/// Share of text cells replaced at full decay.
const DECAY_DENSITY: f32 = 0.08;
const SPARKLE_DENSITY: f32 = 0.012;

/// Stale data frays: a share of text cells, growing with `amount`, turn to static. The
/// pattern changes once a second, so an idle rig stays at one frame a second.
pub fn decay(buf: &mut Buffer, area: Rect, amount: f32, ms: u64) {
    let second = ms / 1_000;
    let p = amount.clamp(0.0, 1.0) * DECAY_DENSITY;
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let Some(cell) = buf.cell_mut((x, y)) else {
                continue;
            };
            if cell.symbol() == " " {
                continue;
            }
            let h = hash(&[x as u64, y as u64, second, 0xDEC4]);
            if unit(h) < p {
                cell.set_char(DECAY_GLYPHS[(h % DECAY_GLYPHS.len() as u64) as usize]);
                cell.fg = theme::DIM;
            }
        }
    }
}

/// Chaotic mode: hex flickers in the empty space.
pub fn sparkle(buf: &mut Buffer, area: Rect, ms: u64) {
    let tick = ms / 100;
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let Some(cell) = buf.cell_mut((x, y)) else {
                continue;
            };
            if cell.symbol() != " " {
                continue;
            }
            let h = hash(&[x as u64, y as u64, tick, 0x5A]);
            if unit(h) < SPARKLE_DENSITY {
                cell.set_char(char::from(HEX[(h % 16) as usize]));
                cell.fg = theme::DIM;
            }
        }
    }
}

/// Every other row sits on a slightly lighter background. Static, so it costs nothing
/// between frames.
pub fn scanlines(buf: &mut Buffer, area: Rect) {
    let area = area.intersection(buf.area);
    for y in (area.top()..area.bottom()).filter(|y| y % 2 == 1) {
        for x in area.left()..area.right() {
            if let Some(cell) = buf.cell_mut((x, y))
                && cell.bg == theme::BG
            {
                cell.bg = theme::SCANLINE;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::Style;

    use super::*;

    #[test]
    fn scanlines_only_touch_the_plain_background() {
        let area = Rect::new(0, 0, 3, 2);
        let mut buf = Buffer::empty(area);
        buf.set_style(area, Style::new().bg(theme::BG));
        buf[(1, 1)].bg = theme::MAGENTA;
        scanlines(&mut buf, area);
        assert_eq!(buf[(0, 0)].bg, theme::BG);
        assert_eq!(buf[(0, 1)].bg, theme::SCANLINE);
        assert_eq!(buf[(1, 1)].bg, theme::MAGENTA);
    }

    #[test]
    fn no_decay_noise_on_fresh_data() {
        let area = Rect::new(0, 0, 20, 5);
        let mut buf = Buffer::empty(area);
        for y in 0..5 {
            buf.set_string(0, y, "x".repeat(20), Style::new());
        }
        let before = buf.clone();
        decay(&mut buf, area, 0.0, 12_345);
        assert_eq!(buf, before);
        decay(&mut buf, area, 1.0, 12_345);
        assert_ne!(buf, before);
    }
}
