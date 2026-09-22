//! A glitch burst: rows tear sideways with a chromatic tint and a few cells turn to
//! static, fading out over the burst.

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::{hash, progress, unit};
use crate::theme;

const STATIC: &[char] = &['▓', '▒', '░', '█', '▚', '▞'];
/// How often the tear pattern changes.
const FRAME_MS: u128 = 40;

pub struct Burst {
    start: Instant,
    duration: Duration,
    strength: f32,
}

impl Burst {
    pub fn new(start: Instant, duration: Duration, strength: f32) -> Self {
        Self {
            start,
            duration,
            strength,
        }
    }

    pub fn done(&self, now: Instant) -> bool {
        progress(self.start, self.duration, now) >= 1.0
    }

    pub fn apply(&self, buf: &mut Buffer, area: Rect, now: Instant) {
        let t = progress(self.start, self.duration, now);
        if t >= 1.0 || area.width < 2 {
            return;
        }
        let intensity = self.strength * (1.0 - t);
        let frame = (now.saturating_duration_since(self.start).as_millis() / FRAME_MS) as u64;
        for y in area.top()..area.bottom() {
            let h = hash(&[y as u64, frame, 0x611]);
            if unit(h) < intensity * 0.35 {
                let shift = 1 + (h >> 8) as u16 % 3;
                let tint = if h & 1 == 0 {
                    theme::MAGENTA
                } else {
                    theme::CYAN
                };
                tear(buf, area, y, shift, (h >> 4) & 1 == 0, tint);
            }
            for x in area.left()..area.right() {
                let h = hash(&[x as u64, y as u64, frame]);
                if unit(h) < intensity * 0.04
                    && let Some(cell) = buf.cell_mut((x, y))
                {
                    cell.set_char(STATIC[(h % STATIC.len() as u64) as usize]);
                    cell.fg = theme::MAGENTA;
                }
            }
        }
    }
}

/// Rotates row `y` of `area` by `shift` cells and tints it.
fn tear(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    shift: u16,
    right: bool,
    tint: ratatui::style::Color,
) {
    let width = area.width;
    let row: Vec<_> = (area.left()..area.right())
        .filter_map(|x| buf.cell((x, y)).cloned())
        .collect();
    if row.len() != width as usize {
        return;
    }
    for (i, cell) in row.into_iter().enumerate() {
        let i = i as u16;
        let to = if right {
            (i + shift) % width
        } else {
            (i + width - shift % width) % width
        };
        if let Some(target) = buf.cell_mut((area.left() + to, y)) {
            *target = cell;
            target.fg = tint;
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::text::Line;
    use ratatui::widgets::Widget;

    use super::*;

    #[test]
    fn leaves_the_buffer_alone_once_finished() {
        let area = Rect::new(0, 0, 10, 3);
        let mut buf = Buffer::empty(area);
        Line::raw("abcdefghij").render(area, &mut buf);
        let untouched = buf.clone();
        let t0 = Instant::now();
        let burst = Burst::new(t0, Duration::from_millis(300), 1.0);
        burst.apply(&mut buf, area, t0 + Duration::from_millis(300));
        assert_eq!(buf, untouched);
    }

    #[test]
    fn tearing_keeps_every_glyph() {
        let area = Rect::new(0, 0, 6, 1);
        let mut buf = Buffer::empty(area);
        Line::raw("abcdef").render(area, &mut buf);
        tear(&mut buf, area, 0, 2, true, theme::CYAN);
        let row: String = (0..6).map(|x| buf[(x, 0)].symbol()).collect();
        assert_eq!(row, "efabcd");
    }
}
