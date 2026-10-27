//! A day/night map: an equirectangular strip of the globe in half-block cells (two
//! pixels to a cell, one above the other), each pixel shaded by land or sea and by how
//! high the sun stands over it, through the three twilight bands into night. The
//! sector and the point under the sun are marked. The sun's position is floored to
//! five minutes: the terminator moves ~1.25° in that time, less than a pixel at any
//! size this is drawn, and redrawing it every frame would fight the app's changed-cells
//! effect for no visual gain.

use std::sync::LazyLock;

use chrono::{DateTime, Duration, DurationRound, Utc};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::sun::{self, Light};
use crate::theme;

/// The land mask: 180×90 cells of 2° each, `#` for land, rows from 90°N down and
/// columns from 180°W east. Rasterized from Natural Earth's public-domain 1:110m land
/// polygons by `tools/landmask.py`.
const LAND: &str = include_str!("land.txt");
const MASK_COLS: usize = 180;
const MASK_ROWS: usize = 90;
/// A pixel counts as land once at least this share of the mask under it is.
const LAND_SHARE: f64 = 0.5;

const STEP: Duration = Duration::minutes(5);

const DAY_LAND: Color = Color::Rgb(0, 160, 100);
const DAY_SEA: Color = Color::Rgb(0, 72, 110);
const NIGHT_LAND: Color = Color::Rgb(30, 38, 62);
/// Night sea is the screen itself, so the dark side reads as empty space.
const NIGHT_SEA: Color = theme::BG;

static MASK: LazyLock<Vec<bool>> = LazyLock::new(|| {
    LAND.lines()
        .flat_map(|row| row.bytes().map(|b| b == b'#'))
        .collect()
});

/// The moment the map is drawn for: `now`, floored to five minutes.
pub fn map_time(now: DateTime<Utc>) -> DateTime<Utc> {
    now.duration_trunc(STEP).unwrap_or(now)
}

fn mask_at(row: usize, col: usize) -> bool {
    MASK.get(row.min(MASK_ROWS - 1) * MASK_COLS + col.min(MASK_COLS - 1))
        .copied()
        .unwrap_or(false)
}

/// The share of the mask that's land inside a lat/lon box. A box smaller than a mask
/// cell takes the cell under its center.
fn land_share(lat_top: f64, lat_bottom: f64, lon_left: f64, lon_right: f64) -> f64 {
    let row_of = |lat: f64| (90.0 - lat) / 2.0;
    let col_of = |lon: f64| (lon + 180.0) / 2.0;
    // Mask cells whose centers fall inside the box.
    let rows = (row_of(lat_top) - 0.5).ceil().max(0.0) as usize
        ..=((row_of(lat_bottom) - 0.5).floor().max(0.0) as usize).min(MASK_ROWS - 1);
    let cols = (col_of(lon_left) - 0.5).ceil().max(0.0) as usize
        ..=((col_of(lon_right) - 0.5).floor().max(0.0) as usize).min(MASK_COLS - 1);
    let (mut land, mut total) = (0, 0);
    for row in rows {
        for col in cols.clone() {
            total += 1;
            land += usize::from(mask_at(row, col));
        }
    }
    if total == 0 {
        let row = row_of((lat_top + lat_bottom) / 2.0) as usize;
        let col = col_of((lon_left + lon_right) / 2.0) as usize;
        return if mask_at(row, col) { 1.0 } else { 0.0 };
    }
    land as f64 / total as f64
}

/// Whether the land mask has land at `(lat, lon)`.
#[cfg(test)]
fn is_land(lat: f64, lon: f64) -> bool {
    land_share(lat, lat, lon, lon) > 0.0
}

/// A pixel's color: land or sea, dimmed through the twilight bands toward night.
fn shade(land: bool, light: Light) -> Color {
    let (day, night) = if land {
        (DAY_LAND, NIGHT_LAND)
    } else {
        (DAY_SEA, NIGHT_SEA)
    };
    let toward_night = match light {
        Light::Day => return day,
        Light::Civil => 0.4,
        Light::Nautical => 0.65,
        Light::Astronomical => 0.85,
        Light::Night => return night,
    };
    theme::mix(day, night, toward_night)
}

/// One cell from its top and bottom pixels. Night sea is left to the screen's own
/// background so the scanlines run through it like everywhere else.
fn cell(top: Color, bottom: Color) -> Span<'static> {
    match (top == NIGHT_SEA, bottom == NIGHT_SEA) {
        (true, true) => Span::raw(" "),
        (false, true) => Span::styled("▀", Style::new().fg(top)),
        (true, false) => Span::styled("▄", Style::new().fg(bottom)),
        (false, false) if top == bottom => Span::styled("█", Style::new().fg(top)),
        (false, false) => Span::styled("▀", Style::new().fg(top).bg(bottom)),
    }
}

/// A marker glyph over whichever of its cell's pixels holds the point.
fn marker(glyph: &'static str, fg: Color, under: Color) -> Span<'static> {
    let style = Style::new().fg(fg).add_modifier(Modifier::BOLD);
    if under == NIGHT_SEA {
        Span::styled(glyph, style)
    } else {
        Span::styled(glyph, style.bg(under))
    }
}

/// The pixel holding `(lat, lon)` on a `cols`×`pixel_rows` grid, as `(pixel_row, col)`.
fn pixel_of(lat: f64, lon: f64, cols: usize, pixel_rows: usize) -> (usize, usize) {
    let row = ((90.0 - lat) / 180.0 * pixel_rows as f64).floor() as usize;
    let col = ((lon + 180.0) / 360.0 * cols as f64).floor() as usize;
    (row.min(pixel_rows - 1), col.min(cols - 1))
}

/// A `cols`×`rows` map for `now`, with the sector's fix (if any) marked. `cols` should
/// run about 4× `rows` to keep the map's proportions: lon/lat span 2:1, and each
/// half-block pixel is about square.
pub fn render(
    cols: usize,
    rows: usize,
    now: DateTime<Utc>,
    sector: Option<(f64, f64)>,
) -> Vec<Line<'static>> {
    if cols == 0 || rows == 0 {
        return Vec::new();
    }
    let sun = sun::subsolar(map_time(now));
    let pixel_rows = rows * 2;
    let lat_step = 180.0 / pixel_rows as f64;
    let lon_step = 360.0 / cols as f64;
    let pixel = |row: usize, col: usize| {
        let lat_top = 90.0 - row as f64 * lat_step;
        let lon_left = -180.0 + col as f64 * lon_step;
        let land =
            land_share(lat_top, lat_top - lat_step, lon_left, lon_left + lon_step) >= LAND_SHARE;
        let light = Light::of(sun::elevation_at(
            lat_top - lat_step / 2.0,
            lon_left + lon_step / 2.0,
            sun,
        ));
        shade(land, light)
    };
    let sun_at = pixel_of(sun.0, sun.1, cols, pixel_rows);
    let sector_at = sector.map(|(lat, lon)| pixel_of(lat, lon, cols, pixel_rows));
    (0..rows)
        .map(|row| {
            let spans: Vec<Span<'static>> = (0..cols)
                .map(|col| {
                    let (top, bottom) = (pixel(row * 2, col), pixel(row * 2 + 1, col));
                    let in_cell = |at: Option<(usize, usize)>| {
                        at.filter(|&(r, c)| r / 2 == row && c == col)
                            .map(|(r, _)| if r % 2 == 0 { top } else { bottom })
                    };
                    // The sector wins a cell it shares with the sun: it's the one to find.
                    if let Some(under) = in_cell(sector_at) {
                        marker("◆", theme::MAGENTA, under)
                    } else if let Some(under) = in_cell(Some(sun_at)) {
                        marker("☼", theme::YELLOW, under)
                    } else {
                        cell(top, bottom)
                    }
                })
                .collect();
            Line::from(spans)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use unicode_width::UnicodeWidthStr;

    use super::*;

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    fn glyphs(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    fn count(lines: &[Line], glyph: &str) -> usize {
        glyphs(lines).iter().map(|l| l.matches(glyph).count()).sum()
    }

    #[test]
    fn the_mask_is_the_size_it_claims() {
        let rows: Vec<&str> = LAND.lines().collect();
        assert_eq!(rows.len(), MASK_ROWS);
        assert!(rows.iter().all(|r| r.len() == MASK_COLS));
        assert!(LAND.bytes().all(|b| matches!(b, b'#' | b'.' | b'\n')));
    }

    #[test]
    fn spot_checks_land_and_sea() {
        assert!(is_land(39.0, -98.0), "Kansas");
        assert!(is_land(52.0, -1.0), "England");
        assert!(is_land(-30.0, 135.0), "Australia");
        assert!(is_land(36.0, 138.0), "Japan");
        assert!(is_land(-75.0, 0.0), "Antarctica");
        assert!(is_land(0.0, 25.0), "Congo");
        assert!(!is_land(0.0, -140.0), "mid Pacific");
        assert!(!is_land(-30.0, -20.0), "mid South Atlantic");
        assert!(!is_land(15.0, 65.0), "Arabian Sea");
        assert!(!is_land(25.0, -90.0), "Gulf of Mexico");
    }

    #[test]
    fn the_map_moves_in_five_minute_steps() {
        assert_eq!(map_time(utc(2026, 6, 21, 12, 7)), utc(2026, 6, 21, 12, 5));
        let a = render(64, 12, utc(2026, 6, 21, 12, 5), None);
        let b = render(64, 12, utc(2026, 6, 21, 12, 9), None);
        assert_eq!(a, b);
        assert_ne!(a, render(64, 12, utc(2026, 6, 21, 13, 0), None));
    }

    #[test]
    fn covers_the_requested_grid_one_column_per_cell() {
        let lines = render(64, 12, utc(2026, 10, 27, 15, 30), Some((37.77, -122.42)));
        assert_eq!(lines.len(), 12);
        for line in glyphs(&lines) {
            assert_eq!(line.width(), 64, "{line}");
        }
    }

    #[test]
    fn marks_the_sector_and_the_sun_once_each() {
        let now = utc(2026, 10, 27, 15, 30);
        let lines = render(64, 12, now, Some((37.77, -122.42)));
        assert_eq!(count(&lines, "◆"), 1);
        assert_eq!(count(&lines, "☼"), 1);
        // San Francisco: 37.77°N is pixel row (90 - 37.77) / 180 * 24 = 6.96 of 24, so
        // cell row 3; -122.42° is column (180 - 122.42) / 360 * 64 = 10.2 of 64.
        let row = &glyphs(&lines)[3];
        assert_eq!(row.chars().position(|c| c == '◆'), Some(10));
        assert_eq!(count(&render(64, 12, now, None), "◆"), 0);
    }

    #[test]
    fn shades_by_the_sun() {
        let now = utc(2026, 3, 20, 12, 0);
        let sun = sun::subsolar(now);
        let at = |lat: f64, lon: f64| Light::of(sun::elevation_at(lat, lon, sun));
        // Equinox noon UTC: lit over Africa, dark over the Pacific, twilight between.
        assert_eq!(at(0.0, 20.0), Light::Day);
        assert_eq!(at(0.0, -170.0), Light::Night);
        assert_eq!(at(0.0, 93.0), Light::Civil);
        assert_eq!(shade(true, Light::Day), DAY_LAND);
        assert_eq!(shade(false, Light::Night), NIGHT_SEA);
        let civil = shade(false, Light::Civil);
        assert!(civil != DAY_SEA && civil != NIGHT_SEA);
    }

    #[test]
    fn empty_grid_is_empty() {
        assert!(render(0, 4, Utc::now(), None).is_empty());
        assert!(render(4, 0, Utc::now(), None).is_empty());
    }
}
