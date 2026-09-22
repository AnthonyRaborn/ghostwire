//! A north-up radar scope: range rings, a rotating sweep, and blips that glow right
//! after the sweep passes them and fade until it comes round again.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::symbols::Marker;
use ratatui::text::Span;
use ratatui::widgets::canvas::{Canvas, Circle, Line as CanvasLine};

use super::text::distance;
use crate::config::Units;
use crate::theme;

/// One full turn of the sweep.
pub const SWEEP_PERIOD_MS: i64 = 4_000;
/// Degrees of afterglow behind the sweep.
const GLOW_DEG: f64 = 120.0;
const RINGS: u32 = 3;

pub struct Blip {
    pub distance_km: f64,
    pub bearing: f64,
    pub glyph: String,
    pub color: Color,
    pub label: Option<String>,
}

/// Sweep angle for a wall-clock time, in degrees clockwise from north.
pub fn sweep_at(ms: i64) -> f64 {
    ms.rem_euclid(SWEEP_PERIOD_MS) as f64 / SWEEP_PERIOD_MS as f64 * 360.0
}

/// The largest scope that fits in `area` and looks round: terminal cells are about
/// twice as tall as they are wide, so it's twice as many columns as rows.
fn scope_area(area: Rect) -> Rect {
    let height = area.height.min(area.width / 2);
    let [_, row, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .areas(area);
    let [_, scope, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(height * 2),
        Constraint::Fill(1),
    ])
    .areas(row);
    scope
}

pub fn draw(
    frame: &mut Frame,
    area: Rect,
    range_km: f64,
    blips: &[Blip],
    sweep: f64,
    units: Units,
    empty_note: Option<&str>,
) {
    let scope = scope_area(area);
    let r = range_km;
    let point = |d: f64, deg: f64| {
        let rad = deg.to_radians();
        (d * rad.sin(), d * rad.cos())
    };
    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .background_color(theme::BG)
        .x_bounds([-r, r])
        .y_bounds([-r, r])
        .paint(|ctx| {
            for ring in 1..=RINGS {
                ctx.draw(&Circle {
                    x: 0.0,
                    y: 0.0,
                    radius: r * f64::from(ring) / f64::from(RINGS),
                    color: theme::DIM,
                });
            }
            ctx.draw(&CanvasLine::new(-r, 0.0, r, 0.0, theme::DIM));
            ctx.draw(&CanvasLine::new(0.0, -r, 0.0, r, theme::DIM));
            for (lag, color) in [
                (0.0, theme::GREEN),
                (4.0, theme::RADAR_TRAIL),
                (8.0, theme::DIM),
            ] {
                let (x, y) = point(r, sweep - lag);
                ctx.draw(&CanvasLine::new(0.0, 0.0, x, y, color));
            }
            ctx.layer();
            for (label, deg) in [("N", 0.0), ("E", 90.0), ("S", 180.0), ("W", 270.0)] {
                let (x, y) = point(r * 0.92, deg);
                ctx.print(x, y, Span::styled(label, Style::new().fg(theme::MUTED)));
            }
            let (x, y) = point(r * 0.99, 135.0);
            ctx.print(
                x,
                y,
                Span::styled(distance(r, units), Style::new().fg(theme::MUTED)),
            );
            if let Some(note) = empty_note {
                let x = -(note.chars().count() as f64) / f64::from(scope.width.max(1)) * r;
                ctx.print(
                    x,
                    -r * 0.25,
                    Span::styled(note.to_string(), Style::new().fg(theme::MUTED)),
                );
            }
            for blip in blips.iter().filter(|b| b.distance_km <= r) {
                let (x, y) = point(blip.distance_km, blip.bearing);
                let behind = (sweep - blip.bearing).rem_euclid(360.0);
                let dim = if behind < GLOW_DEG {
                    behind / GLOW_DEG * 0.7
                } else {
                    0.7
                };
                let color = theme::mix(blip.color, theme::DIM, dim as f32);
                ctx.print(
                    x,
                    y,
                    Span::styled(blip.glyph.clone(), Style::new().fg(color)),
                );
                if let Some(label) = &blip.label {
                    let offset = r * 2.5 / f64::from(scope.width.max(1));
                    ctx.print(
                        x + offset,
                        y,
                        Span::styled(label.clone(), Style::new().fg(color)),
                    );
                }
            }
            // Last, so no blip label can cover the rig's own position.
            ctx.print(0.0, 0.0, Span::styled("◆", Style::new().fg(theme::CYAN)));
        });
    frame.render_widget(canvas, scope);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweep_turns_once_per_period() {
        assert_eq!(sweep_at(0), 0.0);
        assert_eq!(sweep_at(SWEEP_PERIOD_MS / 4), 90.0);
        assert_eq!(sweep_at(SWEEP_PERIOD_MS + 1_000), 90.0);
        assert!(sweep_at(-1_000) >= 0.0);
    }

    #[test]
    fn scope_is_round_and_centered() {
        let scope = scope_area(Rect::new(0, 0, 100, 20));
        assert_eq!((scope.width, scope.height), (40, 20));
        assert_eq!(scope.x, 30);
        let tall = scope_area(Rect::new(0, 0, 30, 40));
        assert_eq!((tall.width, tall.height), (30, 15));
    }
}
