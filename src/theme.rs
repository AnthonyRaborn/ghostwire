use ratatui::style::Color;

use crate::source::Link;

pub const BG: Color = Color::Rgb(7, 8, 15);
pub const TEXT: Color = Color::Rgb(200, 211, 245);
pub const MUTED: Color = Color::Rgb(107, 115, 148);
pub const DIM: Color = Color::Rgb(40, 46, 70);
pub const CYAN: Color = Color::Rgb(0, 240, 255);
pub const BORDER: Color = Color::Rgb(0, 140, 160);
pub const MAGENTA: Color = Color::Rgb(255, 42, 109);
pub const YELLOW: Color = Color::Rgb(245, 211, 0);
pub const ORANGE: Color = Color::Rgb(255, 140, 0);
pub const GREEN: Color = Color::Rgb(5, 255, 161);
pub const RED: Color = Color::Rgb(255, 60, 60);
pub const GHOST: Color = Color::Rgb(150, 130, 255);
/// Every other row's background.
pub const SCANLINE: Color = Color::Rgb(11, 12, 22);
pub const RADAR_TRAIL: Color = Color::Rgb(0, 120, 80);

/// Foreground for a link's label and ticker text.
pub fn link_color(link: &Link) -> Color {
    match link {
        Link::Live => GREEN,
        Link::Ghost => GHOST,
        Link::Pending => CYAN,
        Link::Ice => YELLOW,
        Link::Trace => MAGENTA,
        Link::Flatlined => RED,
        Link::Offline(_) => MUTED,
    }
}

pub fn border_color(link: Option<&Link>) -> Color {
    match link {
        Some(Link::Live) => BORDER,
        Some(Link::Ghost | Link::Pending | Link::Offline(_)) | None => DIM,
        Some(other) => link_color(other),
    }
}

/// Linear blend from `from` toward `to`. Non-RGB colors are returned unchanged.
pub fn mix(from: Color, to: Color, t: f32) -> Color {
    let from = if from == Color::Reset { TEXT } else { from };
    let (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) = (from, to) else {
        return from;
    };
    let t = t.clamp(0.0, 1.0);
    let lerp = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
    Color::Rgb(lerp(r1, r2), lerp(g1, g2), lerp(b1, b2))
}
