//! A north-up radar scope: range rings, a rotating sweep, and blips that glow right
//! after the sweep passes them and fade until it comes round again.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::symbols::Marker;
use ratatui::text::Span;
use ratatui::widgets::canvas::{Canvas, Circle, Line as CanvasLine};

use crate::theme;

/// One full turn of the sweep.
pub const SWEEP_PERIOD_MS: i64 = 4_000;
/// Degrees of afterglow behind the sweep.
const GLOW_DEG: f64 = 120.0;
/// Range rings for a scope with no natural step count of its own (quakes, aircraft).
pub const DEFAULT_RINGS: u32 = 3;

pub struct Blip {
    /// Radial distance from center, in whatever unit the scope's `range` is (km for a
    /// quake/flight scope, forecast hours for a time-based one).
    pub r: f64,
    pub bearing: f64,
    pub glyph: String,
    pub color: Color,
    pub label: Option<String>,
}

/// A filled pie slice from the center out to `r` — a region of intensity rather than a
/// single point, for a source (like the precip nowcast) where "how much" matters as
/// much as "when." Wedges are drawn largest-`r`-first so a farther, wider wedge forms
/// the outer band around a nearer, narrower one instead of hiding it.
pub struct Wedge {
    pub r: f64,
    pub bearing: f64,
    pub half_width_deg: f64,
    pub color: Color,
    pub label: Option<String>,
}

/// Sweep angle for a wall-clock time, in degrees clockwise from north.
pub fn sweep_at(ms: i64) -> f64 {
    ms.rem_euclid(SWEEP_PERIOD_MS) as f64 / SWEEP_PERIOD_MS as f64 * 360.0
}

/// The largest scope that fits in `area` and looks round: terminal cells are about
/// twice as tall as they are wide, so it's twice as many columns as rows. Centered by
/// direct arithmetic rather than a flexed layout, so the leftover space always splits
/// symmetrically instead of however the layout engine happens to round it.
fn scope_area(area: Rect) -> Rect {
    let height = area.height.min(area.width / 2);
    let width = height * 2;
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

/// Everything a scope shows besides its position on screen.
#[derive(Clone, Copy)]
pub struct Scope<'a> {
    pub range: f64,
    pub blips: &'a [Blip],
    pub wedges: &'a [Wedge],
    /// Range rings drawn at even steps out to `range`. Match this to the data's own
    /// natural step count where there is one (e.g. one ring per forecast hour) so the
    /// rings mean something instead of being an arbitrary reference grid.
    pub rings: u32,
    pub sweep: f64,
    /// Printed near the edge of the outermost ring — the scale of the whole scope
    /// (e.g. "300km"). Skip it where that's already said elsewhere (the precip
    /// nowcast's legend and wind line cover it) rather than repeat it on-scope.
    pub range_label: Option<&'a str>,
    pub empty_note: Option<&'a str>,
}

pub fn draw(frame: &mut Frame, area: Rect, scope: &Scope) {
    let Scope {
        range,
        blips,
        wedges,
        rings,
        sweep,
        range_label,
        empty_note,
    } = *scope;
    let scope = scope_area(area);
    let r = range;
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
            for ring in 1..=rings {
                ctx.draw(&Circle {
                    x: 0.0,
                    y: 0.0,
                    radius: r * f64::from(ring) / f64::from(rings),
                    color: theme::DIM,
                });
            }
            ctx.draw(&CanvasLine::new(-r, 0.0, r, 0.0, theme::DIM));
            ctx.draw(&CanvasLine::new(0.0, -r, 0.0, r, theme::DIM));
            // Farthest first, so a nearer/narrower wedge's color shows through its own
            // ring instead of being painted over by a farther one drawn on top.
            let mut by_radius: Vec<&Wedge> = wedges.iter().filter(|w| w.r <= r).collect();
            by_radius.sort_by(|a, b| b.r.total_cmp(&a.r));
            for wedge in by_radius {
                let spokes = ((wedge.half_width_deg * 2.0) / 3.0).ceil().clamp(4.0, 24.0) as usize;
                for i in 0..=spokes {
                    let t = i as f64 / spokes as f64;
                    let deg =
                        wedge.bearing - wedge.half_width_deg + t * (wedge.half_width_deg * 2.0);
                    let (x, y) = point(wedge.r, deg);
                    ctx.draw(&CanvasLine::new(0.0, 0.0, x, y, wedge.color));
                }
            }
            for (lag, color) in [
                (0.0, theme::GREEN),
                (4.0, theme::RADAR_TRAIL),
                (8.0, theme::DIM),
            ] {
                let (x, y) = point(r, sweep - lag);
                ctx.draw(&CanvasLine::new(0.0, 0.0, x, y, color));
            }
            ctx.layer();
            // North-up always, so only North needs marking.
            let (nx, ny) = point(r * 0.92, 0.0);
            ctx.print(nx, ny, Span::styled("N", Style::new().fg(theme::MUTED)));
            if let Some(range_label) = range_label {
                let (x, y) = point(r * 0.99, 135.0);
                ctx.print(
                    x,
                    y,
                    Span::styled(range_label.to_string(), Style::new().fg(theme::MUTED)),
                );
            }
            if let Some(note) = empty_note {
                let x = -(note.chars().count() as f64) / f64::from(scope.width.max(1)) * r;
                ctx.print(
                    x,
                    -r * 0.25,
                    Span::styled(note.to_string(), Style::new().fg(theme::MUTED)),
                );
            }
            for wedge in wedges.iter().filter(|w| w.r <= r) {
                if let Some(label) = &wedge.label {
                    let (x, y) = point(wedge.r, wedge.bearing);
                    ctx.print(
                        x,
                        y,
                        Span::styled(label.clone(), Style::new().fg(theme::TEXT)),
                    );
                }
            }
            for blip in blips.iter().filter(|b| b.r <= r) {
                let (x, y) = point(blip.r, blip.bearing);
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

    #[test]
    fn scope_centers_with_an_odd_leftover() {
        // An odd amount of slack on either axis used to split unevenly under
        // Layout::Fill; direct arithmetic always centers as tightly as integer
        // division allows, off by at most one column/row rather than biased to a side.
        let scope = scope_area(Rect::new(0, 0, 101, 21));
        let (left, right) = (scope.x, 101 - (scope.x + scope.width));
        assert!(left.abs_diff(right) <= 1, "{left} vs {right}");
        let (top, bottom) = (scope.y, 21 - (scope.y + scope.height));
        assert!(top.abs_diff(bottom) <= 1, "{top} vs {bottom}");
    }
}
