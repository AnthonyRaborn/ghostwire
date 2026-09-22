//! A compact day/night terminator sketch: an equirectangular strip of the globe, lit
//! where the sun's up. Quantized to the current hour rather than redrawn continuously —
//! the terminator only really moves ~15°/hour, and re-decrypting it every frame would
//! fight the app's changed-cells effect for no visual gain.

use chrono::{DateTime, Datelike, Timelike, Utc};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::theme;

/// The point on Earth directly under the sun, `(lat, lon)` in degrees — floored to the
/// top of the current hour.
pub fn subsolar(now: DateTime<Utc>) -> (f64, f64) {
    let hour_mark = now
        .date_naive()
        .and_hms_opt(now.hour(), 0, 0)
        .unwrap_or_default()
        .and_utc();
    let day_of_year = f64::from(hour_mark.ordinal());
    // Low-precision solar declination (good to ~1°) — plenty for a decorative sketch,
    // not a navigation instrument.
    let decl = 23.44_f64.to_radians()
        * (((360.0 / 365.0) * (day_of_year - 81.0)).to_radians()).sin();
    // Subsolar longitude: noon local solar time sits under the sun, and that sweeps
    // west 15°/hour as the day turns.
    let lon = (180.0 - f64::from(hour_mark.hour()) * 15.0 + 180.0).rem_euclid(360.0) - 180.0;
    (decl.to_degrees(), lon)
}

/// Whether `(lat, lon)` is in daylight given the subsolar point — the same
/// sun-angle-above-horizon test as [`crate::geo::elevation_deg`], just asking only
/// whether it's positive.
fn lit(lat: f64, lon: f64, subsolar: (f64, f64)) -> bool {
    let (sun_lat, sun_lon) = subsolar;
    let (lat_r, sun_lat_r) = (lat.to_radians(), sun_lat.to_radians());
    let dlon = (lon - sun_lon).to_radians();
    let cos_zenith = lat_r.sin() * sun_lat_r.sin() + lat_r.cos() * sun_lat_r.cos() * dlon.cos();
    cos_zenith > 0.0
}

/// One box in a rough landmass silhouette: a lat/lon rectangle, `(lat_min, lat_max,
/// lon_min, lon_max)`.
type LandBox = (f64, f64, f64, f64);

/// A coarse approximation of the continents as a handful of boxes — not real
/// coastlines, just enough shape (a taper here, a peninsula there) to read as "that's
/// Africa" at a few dozen columns wide. Antarctica's ice cap is included since a blank
/// bottom edge would look like a rendering bug rather than open ocean.
const LANDMASSES: &[LandBox] = &[
    // North America — main mass, tapering down through Central America.
    (25.0, 72.0, -168.0, -52.0),
    (8.0, 25.0, -105.0, -77.0),
    // Greenland.
    (60.0, 83.0, -55.0, -20.0),
    // South America — wide in the north, narrowing toward Patagonia.
    (-20.0, 12.0, -82.0, -34.0),
    (-56.0, -20.0, -75.0, -63.0),
    // Europe.
    (36.0, 71.0, -10.0, 40.0),
    // Africa — narrowing toward the Cape.
    (-10.0, 37.0, -18.0, 52.0),
    (-35.0, -10.0, 10.0, 33.0),
    // Asia, with the Indian subcontinent and Southeast Asia as separate lobes so the
    // main box can stay a simple rectangle.
    (30.0, 78.0, 40.0, 180.0),
    (5.0, 30.0, 68.0, 100.0),
    (-10.0, 20.0, 92.0, 140.0),
    // Australia.
    (-44.0, -10.0, 112.0, 154.0),
    // Antarctica.
    (-90.0, -60.0, -180.0, 180.0),
];

/// Whether `(lat, lon)` falls inside the rough landmass sketch above.
fn is_land(lat: f64, lon: f64) -> bool {
    LANDMASSES
        .iter()
        .any(|&(lat_min, lat_max, lon_min, lon_max)| {
            lat >= lat_min && lat <= lat_max && lon >= lon_min && lon <= lon_max
        })
}

/// A `cols`×`rows` equirectangular sketch: lit/dark for day and night, land drawn
/// heavier than ocean in both, with the sector's fix (if any) marked. `cols` should run
/// roughly 4x `rows` to read as a map — 360°/180° of lon/lat is 2:1, and terminal cells
/// are about twice as tall as wide.
pub fn render(cols: usize, rows: usize, now: DateTime<Utc>, sector: Option<(f64, f64)>) -> Vec<Line<'static>> {
    if cols == 0 || rows == 0 {
        return Vec::new();
    }
    let sun = subsolar(now);
    (0..rows)
        .map(|row| {
            let lat = 90.0 - (row as f64 + 0.5) / rows as f64 * 180.0;
            let lat_step = 180.0 / rows as f64;
            let lon_step = 360.0 / cols as f64;
            let spans: Vec<Span<'static>> = (0..cols)
                .map(|col| {
                    let lon = (col as f64 + 0.5) / cols as f64 * 360.0 - 180.0;
                    let on_sector = sector.is_some_and(|(slat, slon)| {
                        (lat - slat).abs() < lat_step && (lon - slon).abs() < lon_step
                    });
                    if on_sector {
                        return Span::styled("◆", Style::new().fg(theme::MAGENTA));
                    }
                    match (lit(lat, lon, sun), is_land(lat, lon)) {
                        (true, true) => Span::styled("▓", Style::new().fg(theme::GREEN)),
                        (true, false) => Span::styled("·", Style::new().fg(theme::CYAN)),
                        (false, true) => Span::styled("·", Style::new().fg(theme::MUTED)),
                        (false, false) => Span::raw(" "),
                    }
                })
                .collect();
            Line::from(spans)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn subsolar_tracks_the_hour_not_the_minute() {
        let a = Utc.with_ymd_and_hms(2026, 6, 21, 12, 5, 0).unwrap();
        let b = Utc.with_ymd_and_hms(2026, 6, 21, 12, 55, 0).unwrap();
        assert_eq!(subsolar(a), subsolar(b));
    }

    #[test]
    fn subsolar_declination_peaks_near_solstice() {
        // Northern summer solstice: the subsolar point should sit near +23.4° lat.
        let solstice = Utc.with_ymd_and_hms(2026, 6, 21, 12, 0, 0).unwrap();
        let (lat, _) = subsolar(solstice);
        assert!((lat - 23.44).abs() < 1.0, "{lat}");
    }

    #[test]
    fn noon_utc_puts_the_subsolar_point_near_the_prime_meridian() {
        let noon = Utc.with_ymd_and_hms(2026, 3, 20, 12, 0, 0).unwrap();
        let (_, lon) = subsolar(noon);
        assert!(lon.abs() < 8.0, "{lon}");
    }

    #[test]
    fn the_subsolar_point_itself_is_always_lit() {
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 4, 0, 0).unwrap();
        let sun = subsolar(now);
        assert!(lit(sun.0, sun.1, sun));
    }

    #[test]
    fn the_antipode_of_the_subsolar_point_is_dark() {
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 4, 0, 0).unwrap();
        let (lat, lon) = subsolar(now);
        let antipode = (-lat, ((lon + 180.0 + 180.0).rem_euclid(360.0)) - 180.0);
        assert!(!lit(antipode.0, antipode.1, subsolar(now)));
    }

    #[test]
    fn spot_checks_land_and_ocean() {
        assert!(is_land(39.0, -98.0), "Kansas"); // continental US
        assert!(is_land(51.5, -0.1), "London");
        assert!(is_land(-33.9, 151.2), "Sydney");
        assert!(is_land(35.7, 139.7), "Tokyo");
        assert!(is_land(-75.0, 0.0), "Antarctica");
        assert!(!is_land(0.0, -140.0), "mid Pacific");
        assert!(!is_land(-30.0, -20.0), "mid South Atlantic");
        assert!(!is_land(20.0, 65.0), "Arabian Sea");
    }

    #[test]
    fn render_covers_the_requested_grid() {
        let lines = render(8, 4, Utc::now(), None);
        assert_eq!(lines.len(), 4);
        for line in &lines {
            assert_eq!(line.spans.len(), 8);
        }
    }

    #[test]
    fn empty_grid_is_empty() {
        assert!(render(0, 4, Utc::now(), None).is_empty());
        assert!(render(4, 0, Utc::now(), None).is_empty());
    }
}
