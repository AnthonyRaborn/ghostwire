//! Where the sun is, worked out locally from the clock: the subsolar point, its
//! elevation anywhere on Earth, and the next sunrise or sunset. Uses NOAA's
//! fractional-year approximations, good to a minute or two of time and a fraction of a
//! degree, which is far finer than anything drawn from it.

use std::f64::consts::TAU;

use chrono::{DateTime, Datelike, Duration, Timelike, Utc};

/// The sun's center sits this far below the horizon at sunrise and sunset, once
/// refraction and the disc's radius are allowed for.
const HORIZON_DEG: f64 = -0.833;
/// How far ahead to look for the next sunrise or sunset, and how finely.
const SEARCH: Duration = Duration::hours(24);
const SEARCH_STEP: Duration = Duration::minutes(2);

/// The point directly under the sun, `(lat, lon)` in degrees.
pub fn subsolar(t: DateTime<Utc>) -> (f64, f64) {
    let hours =
        f64::from(t.hour()) + f64::from(t.minute()) / 60.0 + f64::from(t.second()) / 3_600.0;
    let days_in_year = if t.date_naive().leap_year() {
        366.0
    } else {
        365.0
    };
    let g = TAU / days_in_year * (f64::from(t.ordinal()) - 1.0 + (hours - 12.0) / 24.0);
    let decl = 0.006918 - 0.399912 * g.cos() + 0.070257 * g.sin() - 0.006758 * (2.0 * g).cos()
        + 0.000907 * (2.0 * g).sin()
        - 0.002697 * (3.0 * g).cos()
        + 0.00148 * (3.0 * g).sin();
    // Equation of time, in minutes: how far the true sun runs ahead of clock noon.
    let eqtime = 229.18
        * (0.000075 + 0.001868 * g.cos()
            - 0.032077 * g.sin()
            - 0.014615 * (2.0 * g).cos()
            - 0.040849 * (2.0 * g).sin());
    // Solar noon sweeps west 15° an hour, from the prime meridian at 12:00 true solar time.
    let lon = -15.0 * (hours - 12.0 + eqtime / 60.0);
    (decl.to_degrees(), (lon + 180.0).rem_euclid(360.0) - 180.0)
}

/// The sun's elevation above the horizon at `(lat, lon)`, in degrees, given where
/// it's overhead.
pub fn elevation_at(lat: f64, lon: f64, subsolar: (f64, f64)) -> f64 {
    let (sun_lat, sun_lon) = subsolar;
    let (lat, sun_lat) = (lat.to_radians(), sun_lat.to_radians());
    let dlon = (lon - sun_lon).to_radians();
    let cos_zenith = lat.sin() * sun_lat.sin() + lat.cos() * sun_lat.cos() * dlon.cos();
    cos_zenith.clamp(-1.0, 1.0).asin().to_degrees()
}

pub fn elevation(lat: f64, lon: f64, t: DateTime<Utc>) -> f64 {
    elevation_at(lat, lon, subsolar(t))
}

/// How lit a place is, by the standard twilight bands of the sun's elevation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Light {
    Day,
    /// Sun 0-6° below the horizon: outdoors is still readable without lights.
    Civil,
    /// 6-12° below: the horizon at sea is still visible.
    Nautical,
    /// 12-18° below: the sky is all but dark.
    Astronomical,
    Night,
}

impl Light {
    pub fn of(elevation_deg: f64) -> Self {
        match elevation_deg {
            e if e >= 0.0 => Light::Day,
            e if e >= -6.0 => Light::Civil,
            e if e >= -12.0 => Light::Nautical,
            e if e >= -18.0 => Light::Astronomical,
            _ => Light::Night,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Rise(DateTime<Utc>),
    Set(DateTime<Utc>),
    /// No sunrise or sunset in the next day: polar day or polar night.
    StaysUp,
    StaysDown,
}

/// The next sunrise or sunset at `(lat, lon)` after `t`, to within a few seconds.
pub fn next_event(lat: f64, lon: f64, t: DateTime<Utc>) -> Event {
    let up = |at| elevation(lat, lon, at) > HORIZON_DEG;
    let was_up = up(t);
    let mut prev = t;
    while prev - t < SEARCH {
        let next = prev + SEARCH_STEP;
        if up(next) != was_up {
            // Narrow the crossing down from two minutes to a few seconds.
            let (mut lo, mut hi) = (prev, next);
            while hi - lo > Duration::seconds(5) {
                let mid = lo + (hi - lo) / 2;
                if up(mid) == was_up {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            return if was_up {
                Event::Set(hi)
            } else {
                Event::Rise(hi)
            };
        }
        prev = next;
    }
    if was_up {
        Event::StaysUp
    } else {
        Event::StaysDown
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    fn minutes_apart(a: DateTime<Utc>, b: DateTime<Utc>) -> i64 {
        (a - b).num_minutes().abs()
    }

    #[test]
    fn declination_follows_the_seasons() {
        let (june, _) = subsolar(utc(2026, 6, 21, 12, 0));
        let (dec, _) = subsolar(utc(2026, 12, 21, 12, 0));
        let (march, _) = subsolar(utc(2026, 3, 20, 12, 0));
        assert!((june - 23.44).abs() < 0.3, "{june}");
        assert!((dec + 23.44).abs() < 0.3, "{dec}");
        assert!(march.abs() < 0.5, "{march}");
    }

    #[test]
    fn the_equation_of_time_moves_solar_noon() {
        // Early November the true sun runs ~16 minutes ahead of the clock, so at 12:00 UTC
        // it's already about 4° west of Greenwich; mid-February it lags ~14 minutes behind.
        let (_, nov) = subsolar(utc(2026, 11, 3, 12, 0));
        let (_, feb) = subsolar(utc(2026, 2, 11, 12, 0));
        assert!((nov + 4.1).abs() < 0.3, "{nov}");
        assert!((feb - 3.6).abs() < 0.3, "{feb}");
    }

    #[test]
    fn elevation_is_overhead_at_the_subsolar_point_and_negative_opposite() {
        let t = utc(2026, 9, 1, 4, 0);
        let (lat, lon) = subsolar(t);
        assert!((elevation(lat, lon, t) - 90.0).abs() < 1e-6);
        assert!((elevation(-lat, lon + 180.0, t) + 90.0).abs() < 1e-6);
    }

    #[test]
    fn san_francisco_sunrise_and_sunset_match_noaa() {
        // NOAA's solar calculator, 37.77, -122.42 on 2026-06-21: sunrise 05:48 PDT
        // (12:48 UTC), sunset 20:35 PDT (03:35 UTC on the 22nd).
        let (lat, lon) = (37.77, -122.42);
        match next_event(lat, lon, utc(2026, 6, 21, 10, 0)) {
            Event::Rise(at) => assert!(minutes_apart(at, utc(2026, 6, 21, 12, 48)) <= 2, "{at}"),
            other => panic!("{other:?}"),
        }
        match next_event(lat, lon, utc(2026, 6, 21, 20, 0)) {
            Event::Set(at) => assert!(minutes_apart(at, utc(2026, 6, 22, 3, 35)) <= 2, "{at}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn polar_summer_and_winter_have_no_sunset_or_sunrise() {
        assert_eq!(
            next_event(80.0, 0.0, utc(2026, 6, 21, 0, 0)),
            Event::StaysUp
        );
        assert_eq!(
            next_event(80.0, 0.0, utc(2026, 12, 21, 0, 0)),
            Event::StaysDown
        );
    }
}
