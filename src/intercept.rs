//! Priority intercepts: notable changes between a source's previous reading and its new
//! one. Thresholds re-arm only after the value falls back a little (hysteresis), so a
//! price hovering at 3% doesn't fire on every refresh.

use std::collections::HashSet;

use chrono::{DateTime, Utc};

use crate::config::Sector;
use crate::geo;
use crate::reading::{Quote, Reading};
use crate::ui::text::distance;

pub const PRICE_MOVE_PCT: f64 = 3.0;
const PRICE_REARM_PCT: f64 = 2.5;
pub const QUAKE_MAG: f64 = 4.0;
/// Older quakes are history, not news, even if this is the first time the rig sees them.
const QUAKE_RECENT_HOURS: i64 = 6;
pub const STORM_KP: f64 = 5.0;
const STORM_REARM_KP: f64 = 4.5;

/// What to announce, one line per event. Empty when nothing notable happened.
pub fn detect(
    old: Option<&Reading>,
    new: &Reading,
    sector: &Sector,
    now: DateTime<Utc>,
) -> Vec<String> {
    match new {
        Reading::Stocks(quotes) | Reading::Crypto(quotes) => {
            let old = match old {
                Some(Reading::Stocks(q) | Reading::Crypto(q)) => q.as_slice(),
                _ => &[],
            };
            price_moves(old, quotes)
        }
        Reading::Quakes(quakes) => {
            let seen: HashSet<&str> = match old {
                Some(Reading::Quakes(q)) => q.iter().map(|q| q.id.as_str()).collect(),
                _ => HashSet::new(),
            };
            quakes
                .iter()
                .filter(|q| !seen.contains(q.id.as_str()))
                .filter(|q| q.mag >= QUAKE_MAG)
                .filter(|q| (now - q.time).num_hours() < QUAKE_RECENT_HOURS)
                .filter_map(|q| {
                    let d = q.distance_km.filter(|d| *d <= sector.radius_km)?;
                    let bearing = q.bearing.map(geo::compass).unwrap_or("");
                    Some(format!(
                        "M{:.1} QUAKE {} {bearing}",
                        q.mag,
                        distance(d, sector.units)
                    ))
                })
                .collect()
        }
        Reading::Swpc(space) => {
            let armed = match old {
                Some(Reading::Swpc(o)) => o.kp < STORM_REARM_KP,
                _ => true,
            };
            if armed && space.kp >= STORM_KP {
                vec![format!(
                    "GEOMAGNETIC STORM Kp {:.1} G{}",
                    space.kp, space.scales.g
                )]
            } else {
                Vec::new()
            }
        }
        // With no earlier catalog to compare against, everything would look new.
        Reading::Kev(vulns) => match old {
            Some(Reading::Kev(o)) => {
                let seen: HashSet<&str> = o.iter().map(|v| v.cve.as_str()).collect();
                vulns
                    .iter()
                    .filter(|v| !seen.contains(v.cve.as_str()))
                    .map(|v| format!("NEW KEV {} {} {}", v.cve, v.vendor, v.product))
                    .collect()
            }
            _ => Vec::new(),
        },
        Reading::Weather(_) | Reading::Hn(_) | Reading::OpenSky(_) => Vec::new(),
    }
}

fn price_moves(old: &[Quote], new: &[Quote]) -> Vec<String> {
    new.iter()
        .filter(|q| q.change_pct.abs() >= PRICE_MOVE_PCT)
        .filter(|q| {
            old.iter()
                .find(|o| o.symbol == q.symbol)
                .is_none_or(|o| o.change_pct.abs() < PRICE_REARM_PCT)
        })
        .map(|q| {
            let arrow = if q.change_pct >= 0.0 { '▲' } else { '▼' };
            format!("{} {arrow}{:.1}%", q.symbol, q.change_pct.abs())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;
    use crate::reading::{Quake, Scales, SpaceWeather, Vuln};

    fn quote(symbol: &str, change_pct: f64) -> Quote {
        Quote {
            symbol: symbol.into(),
            price: 1.0,
            change_pct,
            spark: Vec::new(),
        }
    }

    fn quake(id: &str, mag: f64, distance_km: f64, age_hours: i64) -> Quake {
        Quake {
            id: id.into(),
            mag,
            place: "x".into(),
            time: Utc::now() - Duration::hours(age_hours),
            depth_km: 5.0,
            distance_km: Some(distance_km),
            bearing: Some(45.0),
            tsunami: false,
        }
    }

    fn space(kp: f64) -> Reading {
        Reading::Swpc(SpaceWeather {
            kp,
            kp_history: Vec::new(),
            xray_flux: None,
            scales: Scales { g: 1, s: 0, r: 0 },
        })
    }

    fn vuln(cve: &str) -> Vuln {
        Vuln {
            cve: cve.into(),
            vendor: "Acme".into(),
            product: "Router".into(),
            name: String::new(),
            added: Utc::now().date_naive(),
            ransomware: false,
        }
    }

    #[test]
    fn price_moves_fire_on_crossing_with_hysteresis() {
        let sector = Sector::default();
        let now = Utc::now();
        let calm = Reading::Crypto(vec![quote("BTC", 1.0)]);
        let jump = Reading::Crypto(vec![quote("BTC", 3.4)]);
        let dip = Reading::Crypto(vec![quote("BTC", -3.1)]);
        let hover = Reading::Crypto(vec![quote("BTC", 2.8)]);
        assert_eq!(detect(Some(&calm), &jump, &sector, now), ["BTC ▲3.4%"]);
        assert!(detect(Some(&jump), &dip, &sector, now).is_empty());
        // 2.8% hasn't fallen below the re-arm line, so a return to 3% stays quiet.
        assert!(detect(Some(&hover), &jump, &sector, now).is_empty());
        assert_eq!(detect(None, &dip, &sector, now), ["BTC ▼3.1%"]);
    }

    #[test]
    fn only_new_recent_nearby_strong_quakes() {
        let sector = Sector::default();
        let now = Utc::now();
        let old = Reading::Quakes(vec![quake("seen", 5.0, 10.0, 0)]);
        let new = Reading::Quakes(vec![
            quake("seen", 5.0, 10.0, 0),
            quake("small", 3.9, 10.0, 0),
            quake("far", 6.0, 5_000.0, 0),
            quake("stale", 5.0, 10.0, 12),
            quake("hit", 4.4, 38.0, 1),
        ]);
        assert_eq!(
            detect(Some(&old), &new, &sector, now),
            ["M4.4 QUAKE 38km NE"]
        );
    }

    #[test]
    fn storms_fire_once_per_crossing() {
        let sector = Sector::default();
        let now = Utc::now();
        assert_eq!(
            detect(None, &space(5.33), &sector, now),
            ["GEOMAGNETIC STORM Kp 5.3 G1"]
        );
        assert!(detect(Some(&space(5.0)), &space(6.0), &sector, now).is_empty());
        assert!(detect(Some(&space(4.67)), &space(5.0), &sector, now).is_empty());
        assert_eq!(
            detect(Some(&space(4.0)), &space(5.0), &sector, now).len(),
            1
        );
    }

    #[test]
    fn new_kev_entries_need_an_earlier_catalog() {
        let sector = Sector::default();
        let now = Utc::now();
        let old = Reading::Kev(vec![vuln("CVE-1")]);
        let new = Reading::Kev(vec![vuln("CVE-2"), vuln("CVE-1")]);
        assert_eq!(
            detect(Some(&old), &new, &sector, now),
            ["NEW KEV CVE-2 Acme Router"]
        );
        assert!(detect(None, &new, &sector, now).is_empty());
    }
}
