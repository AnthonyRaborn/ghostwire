//! 256-color fallback for terminals without truecolor support.
//!
//! The theme is authored entirely in RGB. Where a terminal advertises truecolor via
//! `COLORTERM`, RGB is sent straight through; everywhere else (including terminals that
//! say nothing, which is most of them) colors are downsampled to the nearest xterm-256
//! index, a palette every terminal from the last twenty years understands.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    TrueColor,
    Indexed256,
}

impl Depth {
    pub fn detect() -> Self {
        Self::from_env(|name| std::env::var(name).ok())
    }

    fn from_env(get: impl Fn(&str) -> Option<String>) -> Self {
        match get("COLORTERM").as_deref() {
            Some("truecolor" | "24bit") => Depth::TrueColor,
            _ => Depth::Indexed256,
        }
    }
}

/// Downsamples every RGB color in `area` to the nearest xterm-256 index, in place. A
/// no-op at `TrueColor`, so callers can run it unconditionally.
pub fn downsample(buf: &mut Buffer, area: Rect, depth: Depth) {
    if depth == Depth::TrueColor {
        return;
    }
    let area = area.intersection(buf.area);
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.fg = to_256(cell.fg);
                cell.bg = to_256(cell.bg);
            }
        }
    }
}

fn to_256(color: Color) -> Color {
    match color {
        Color::Rgb(r, g, b) => Color::Indexed(nearest_256(r, g, b)),
        other => other,
    }
}

/// Nearest xterm-256 palette index (16-231 color cube, 232-255 grayscale ramp) by
/// squared Euclidean distance — the conversion most terminal tools use.
fn nearest_256(r: u8, g: u8, b: u8) -> u8 {
    const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let cube_index = |v: u8| -> u8 {
        if v < 48 {
            0
        } else if v < 115 {
            1
        } else {
            (((u16::from(v) - 35) / 40).min(5)) as u8
        }
    };
    let (qr, qg, qb) = (cube_index(r), cube_index(g), cube_index(b));
    let cube_code = 16 + 36 * qr + 6 * qg + qb;
    let cube_rgb = (CUBE[qr as usize], CUBE[qg as usize], CUBE[qb as usize]);

    let gray_avg = ((u16::from(r) + u16::from(g) + u16::from(b)) / 3) as u8;
    let gray_level = if gray_avg > 238 {
        23
    } else {
        gray_avg.saturating_sub(3) / 10
    };
    let gray_value = 8 + 10 * gray_level;
    let gray_code = 232 + gray_level;

    let dist = |(cr, cg, cb): (u8, u8, u8)| -> u32 {
        let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).pow(2) as u32;
        d(r, cr) + d(g, cg) + d(b, cb)
    };
    if dist(cube_rgb) <= dist((gray_value, gray_value, gray_value)) {
        cube_code
    } else {
        gray_code
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truecolor_only_from_colorterm() {
        assert_eq!(
            Depth::from_env(|_| Some("truecolor".into())),
            Depth::TrueColor
        );
        assert_eq!(Depth::from_env(|_| Some("24bit".into())), Depth::TrueColor);
        assert_eq!(Depth::from_env(|_| None), Depth::Indexed256);
        assert_eq!(
            Depth::from_env(|_| Some("256color".into())),
            Depth::Indexed256
        );
    }

    #[test]
    fn maps_primaries_to_expected_cube_corners() {
        assert_eq!(nearest_256(0, 0, 0), 16);
        assert_eq!(nearest_256(255, 255, 255), 231);
        assert_eq!(nearest_256(255, 0, 0), 196);
        assert_eq!(nearest_256(0, 255, 0), 46);
        assert_eq!(nearest_256(0, 0, 255), 21);
    }

    #[test]
    fn grays_land_on_the_ramp() {
        assert_eq!(nearest_256(128, 128, 128), 244);
    }

    #[test]
    fn leaves_non_rgb_colors_alone() {
        let area = Rect::new(0, 0, 1, 1);
        let mut buf = Buffer::empty(area);
        buf[(0, 0)].fg = Color::Reset;
        downsample(&mut buf, area, Depth::Indexed256);
        assert_eq!(buf[(0, 0)].fg, Color::Reset);
    }

    #[test]
    fn truecolor_depth_leaves_buffer_untouched() {
        let area = Rect::new(0, 0, 1, 1);
        let mut buf = Buffer::empty(area);
        buf[(0, 0)].fg = Color::Rgb(1, 2, 3);
        downsample(&mut buf, area, Depth::TrueColor);
        assert_eq!(buf[(0, 0)].fg, Color::Rgb(1, 2, 3));
    }

    #[test]
    fn indexed_downsamples_rgb() {
        let area = Rect::new(0, 0, 1, 1);
        let mut buf = Buffer::empty(area);
        buf[(0, 0)].fg = Color::Rgb(0, 240, 255); // theme::CYAN
        downsample(&mut buf, area, Depth::Indexed256);
        assert!(matches!(buf[(0, 0)].fg, Color::Indexed(_)));
    }
}
