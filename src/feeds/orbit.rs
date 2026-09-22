//! ORBIT, folded into SKYTRAFFIC: the ISS's current position from wheretheiss.at, a
//! free keyless tracker. It's the only object that API covers today, so there's just
//! the one contact — but the reading stays a `Vec` in case that changes.

use std::time::Duration;

use serde::Deserialize;

use super::http::{get_text, parse_json};
use super::{Feed, FetchError};
use crate::config::Config;
use crate::geo;
use crate::reading::{Reading, Satellite};
use crate::source::SourceId;

/// ISS (ZARYA)'s NORAD catalog number — the only satellite wheretheiss.at tracks.
const ISS_NORAD_ID: u32 = 25544;

pub struct Orbit {
    fix: Option<(f64, f64)>,
}

impl Orbit {
    pub fn new(config: &Config) -> Self {
        Self {
            fix: config.sector.fix(),
        }
    }
}

impl Feed for Orbit {
    fn source(&self) -> SourceId {
        SourceId::Orbit
    }

    fn interval(&self) -> Duration {
        // The ISS moves ~7.7 km/s; even a minute old it's still a fair position.
        Duration::from_secs(60)
    }

    async fn fetch(&self, http: &reqwest::Client) -> Result<Reading, FetchError> {
        let Some(here) = self.fix else {
            return Err(FetchError::NotConfigured("set [sector] lat/lon".into()));
        };
        let url = format!("https://api.wheretheiss.at/v1/satellites/{ISS_NORAD_ID}");
        let text = get_text(http.get(url)).await?;
        parse(&text, here)
            .map(Reading::Orbit)
            .map_err(FetchError::Failed)
    }
}

#[derive(Deserialize)]
struct Position {
    name: String,
    latitude: f64,
    longitude: f64,
    altitude: f64,
    velocity: f64,
    visibility: String,
}

pub fn parse(text: &str, here: (f64, f64)) -> Result<Vec<Satellite>, String> {
    let p: Position = parse_json(text)?;
    let distance_km = geo::distance_km(here.0, here.1, p.latitude, p.longitude);
    Ok(vec![Satellite {
        name: p.name.to_uppercase(),
        altitude_km: p.altitude,
        velocity_kmh: p.velocity,
        sunlit: p.visibility.eq_ignore_ascii_case("daylight"),
        distance_km,
        bearing: geo::bearing_deg(here.0, here.1, p.latitude, p.longitude),
        elevation_deg: geo::elevation_deg(distance_km, p.altitude),
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    const POSITION: &str = include_str!("../../tests/fixtures/wheretheiss_iss.json");
    const HERE: (f64, f64) = (37.7749, -122.4194);

    #[test]
    fn parses_a_position_near_overhead() {
        let sats = parse(POSITION, HERE).unwrap();
        assert_eq!(sats.len(), 1);
        let s = &sats[0];
        assert_eq!(s.name, "ISS");
        assert!((s.altitude_km - 417.34).abs() < 0.01);
        assert!((s.velocity_kmh - 27_612.4).abs() < 0.1);
        assert!(s.sunlit);
        // The fixture puts it a few km away, well above the horizon.
        assert!(s.distance_km < 50.0, "{}", s.distance_km);
        assert!(s.elevation_deg > 60.0, "{}", s.elevation_deg);
    }

    #[test]
    fn far_away_reads_as_below_the_horizon() {
        // Antipodal point: as far from the fixture's ISS position as possible.
        let sats = parse(POSITION, (-37.9012, 58.0124)).unwrap();
        assert!(sats[0].elevation_deg < 0.0);
    }

    #[tokio::test]
    async fn offline_without_a_sector_fix() {
        let feed = Orbit::new(&Config::default());
        let result = feed.fetch(&reqwest::Client::new()).await;
        assert!(matches!(result, Err(FetchError::NotConfigured(_))));
    }

    #[test]
    fn rejects_a_broken_payload() {
        assert!(parse("{}", HERE).is_err());
    }
}
