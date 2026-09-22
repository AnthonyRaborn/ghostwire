//! Scramble-then-resolve. Cells that changed since the snapshot (or every non-blank cell,
//! with no snapshot) show churning glyphs until their own reveal time, which is mostly
//! random with a left-to-right sweep.

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::{Snapshot, hash, progress, unit};
use crate::theme;

/// Half-width katakana are one column wide, like the hex around them.
const GLYPHS: &[char] = &[
    'ｱ', 'ｲ', 'ｳ', 'ｴ', 'ｵ', 'ｶ', 'ｷ', 'ｸ', 'ｹ', 'ｺ', 'ｻ', 'ｼ', 'ｽ', 'ｾ', 'ｿ', 'ﾀ', 'ﾁ', 'ﾂ', 'ﾃ',
    'ﾄ', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F', '#', '$',
    '%', '&', '*', '+', '=', '<', '>',
];
/// How often a scrambled cell changes glyph.
const CHURN_MS: u128 = 60;
/// Share of the reveal order that comes from the left-to-right sweep.
const SWEEP: f32 = 0.35;

pub struct Decrypt {
    start: Instant,
    duration: Duration,
    before: Option<Snapshot>,
}

impl Decrypt {
    pub fn new(start: Instant, duration: Duration, before: Option<Snapshot>) -> Self {
        Self {
            start,
            duration,
            before,
        }
    }

    pub fn done(&self, now: Instant) -> bool {
        progress(self.start, self.duration, now) >= 1.0
    }

    pub fn apply(&self, buf: &mut Buffer, area: Rect, now: Instant) {
        let t = progress(self.start, self.duration, now);
        if t >= 1.0 || area.is_empty() {
            return;
        }
        let churn = (now.saturating_duration_since(self.start).as_millis() / CHURN_MS) as u64;
        for (row, y) in (area.top()..area.bottom()).enumerate() {
            for (col, x) in (area.left()..area.right()).enumerate() {
                let Some(cell) = buf.cell_mut((x, y)) else {
                    continue;
                };
                let symbol = cell.symbol();
                if symbol == " " {
                    continue;
                }
                let index = row * area.width as usize + col;
                let changed = self
                    .before
                    .as_ref()
                    .is_none_or(|b| b.changed(area, index, symbol));
                if !changed {
                    continue;
                }
                let sweep = col as f32 / area.width as f32;
                let reveal_at =
                    SWEEP * sweep + (1.0 - SWEEP) * unit(hash(&[x as u64, y as u64, 0xDEC]));
                if t < reveal_at {
                    let h = hash(&[x as u64, y as u64, churn]);
                    let glyph = GLYPHS[(h % GLYPHS.len() as u64) as usize];
                    cell.set_char(glyph);
                    cell.fg = if h & 1 == 0 {
                        theme::CYAN
                    } else {
                        theme::GREEN
                    };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::text::Line;
    use ratatui::widgets::Widget;

    use super::*;

    fn filled(text: &str, area: Rect) -> Buffer {
        let mut buf = Buffer::empty(area);
        Line::raw(text).render(area, &mut buf);
        buf
    }

    fn row(buf: &Buffer) -> String {
        (0..buf.area.width).map(|x| buf[(x, 0)].symbol()).collect()
    }

    #[test]
    fn scrambles_only_changed_cells_and_then_resolves() {
        let area = Rect::new(0, 0, 12, 1);
        let before = Snapshot::take(&filled("BTC   61,204", area), area, None);
        let t0 = Instant::now();
        let d = Decrypt::new(t0, Duration::from_millis(700), Some(before));

        let mut buf = filled("BTC   61,377", area);
        d.apply(&mut buf, area, t0);
        let scrambled = row(&buf);
        assert!(scrambled.starts_with("BTC   61,"), "{scrambled}");
        assert_ne!(scrambled, "BTC   61,377");

        let mut buf = filled("BTC   61,377", area);
        d.apply(&mut buf, area, t0 + Duration::from_millis(700));
        assert_eq!(row(&buf), "BTC   61,377");
        assert!(d.done(t0 + Duration::from_millis(700)));
    }

    #[test]
    fn without_a_snapshot_everything_but_blanks_scrambles() {
        let area = Rect::new(0, 0, 8, 1);
        let t0 = Instant::now();
        let d = Decrypt::new(t0, Duration::from_millis(500), None);
        let mut buf = filled("AB    CD", area);
        d.apply(&mut buf, area, t0);
        let out: Vec<char> = row(&buf).chars().collect();
        assert_eq!(out.len(), 8);
        assert!(out[2..6].iter().all(|c| *c == ' '));
        assert_ne!(out.iter().collect::<String>(), "AB    CD");
    }
}
