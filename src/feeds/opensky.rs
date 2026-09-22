//! SKYTRAFFIC: airborne aircraft within `flight_radius_km`, from the OpenSky Network's
//! anonymous API. Anonymous access gets 400 credits a day and a box up to 25 square
//! degrees costs 1 credit, so it polls every 5 minutes and retries slowly.

use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use super::http::{get_text, parse_json};
use super::{Feed, FetchError};
use crate::config::Config;
use crate::geo;
use crate::reading::{Contact, Reading};
use crate::source::SourceId;

const KM_PER_DEGREE: f64 = 111.2;

pub struct OpenSky {
    fix: Option<(f64, f64)>,
    radius_km: f64,
}

impl OpenSky {
    pub fn new(config: &Config) -> Self {
        Self {
            fix: config.sector.fix(),
            radius_km: config.sector.flight_radius_km,
        }
    }
}

impl Feed for OpenSky {
    fn source(&self) -> SourceId {
        SourceId::OpenSky
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(5 * 60)
    }

    fn retry_base(&self) -> Duration {
        Duration::from_secs(60)
    }

    async fn fetch(&self, http: &reqwest::Client) -> Result<Reading, FetchError> {
        let Some((lat, lon)) = self.fix else {
            return Err(FetchError::NotConfigured("set [sector] lat/lon".into()));
        };
        let [lamin, lomin, lamax, lomax] = bounding_box(lat, lon, self.radius_km);
        let url = format!(
            "https://opensky-network.org/api/states/all\
             ?lamin={lamin:.4}&lomin={lomin:.4}&lamax={lamax:.4}&lomax={lomax:.4}"
        );
        let text = get_text(http.get(url)).await?;
        parse(&text, (lat, lon), self.radius_km)
            .map(Reading::OpenSky)
            .map_err(FetchError::Failed)
    }
}

/// `[lamin, lomin, lamax, lomax]` around the point. Doesn't wrap the antimeridian.
fn bounding_box(lat: f64, lon: f64, radius_km: f64) -> [f64; 4] {
    let dlat = radius_km / KM_PER_DEGREE;
    let dlon = radius_km / (KM_PER_DEGREE * lat.to_radians().cos().max(0.01));
    [
        (lat - dlat).max(-90.0),
        (lon - dlon).max(-180.0),
        (lat + dlat).min(90.0),
        (lon + dlon).min(180.0),
    ]
}

#[derive(Deserialize)]
struct States {
    /// Each state vector is a positional array; `null` when nothing is in the box.
    states: Option<Vec<Vec<Value>>>,
}

// State vector fields, per the OpenSky REST docs.
const ICAO24: usize = 0;
const CALLSIGN: usize = 1;
const LON: usize = 5;
const LAT: usize = 6;
const BARO_ALTITUDE: usize = 7;
const ON_GROUND: usize = 8;
const VELOCITY: usize = 9;
const TRUE_TRACK: usize = 10;
const GEO_ALTITUDE: usize = 13;

/// Airborne contacts inside the radius (the box's corners stick out past it), nearest
/// first.
pub fn parse(text: &str, here: (f64, f64), radius_km: f64) -> Result<Vec<Contact>, String> {
    let states = parse_json::<States>(text)?.states.unwrap_or_default();
    let mut contacts: Vec<Contact> = states
        .iter()
        .filter_map(|s| {
            let num = |i: usize| s.get(i).and_then(Value::as_f64);
            if s.get(ON_GROUND).and_then(Value::as_bool).unwrap_or(false) {
                return None;
            }
            let (lat, lon) = (num(LAT)?, num(LON)?);
            let distance_km = geo::distance_km(here.0, here.1, lat, lon);
            if distance_km > radius_km {
                return None;
            }
            let icao24 = s.get(ICAO24)?.as_str()?;
            let callsign = s
                .get(CALLSIGN)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .map_or_else(|| icao24.to_uppercase(), str::to_string);
            Some(Contact {
                callsign,
                altitude_m: num(BARO_ALTITUDE).or(num(GEO_ALTITUDE)),
                speed_ms: num(VELOCITY),
                heading: num(TRUE_TRACK),
                distance_km,
                bearing: geo::bearing_deg(here.0, here.1, lat, lon),
            })
        })
        .collect();
    contacts.sort_by(|a, b| a.distance_km.total_cmp(&b.distance_km));
    Ok(contacts)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATES: &str = include_str!("../../tests/fixtures/opensky_states.json");
    const SF: (f64, f64) = (37.77, -122.42);

    #[test]
    fn keeps_airborne_contacts_inside_the_radius() {
        let contacts = parse(STATES, SF, 150.0).unwrap();
        assert!(!contacts.is_empty());
        assert!(contacts.iter().all(|c| c.distance_km <= 150.0));
        assert!(
            contacts
                .windows(2)
                .all(|w| w[0].distance_km <= w[1].distance_km)
        );
        assert!(contacts.iter().all(|c| c.callsign == c.callsign.trim()));
        assert!(parse(STATES, SF, 1.0).unwrap().len() < contacts.len());
    }

    #[test]
    fn handles_an_empty_sky_and_odd_rows() {
        assert!(
            parse(r#"{"time":0,"states":null}"#, SF, 150.0)
                .unwrap()
                .is_empty()
        );
        let text = r#"{"time":0,"states":[
            ["abc123", "   ", "US", 0, 0, -122.4, 37.8, 1000.0, false, 100.0, 90.0, 0, null, 1010.0, null, false, 0],
            ["def456", "UAL1", "US", 0, 0, -122.4, 37.8, 0.0, true, 0.0, 0.0, 0, null, 0.0, null, false, 0],
            ["ghi789", "NOPOS", "US", 0, 0, null, null, 1000.0, false, 100.0, 90.0, 0, null, null, null, false, 0]
        ]}"#;
        let contacts = parse(text, SF, 150.0).unwrap();
        assert_eq!(contacts.len(), 1);
        assert_eq!(contacts[0].callsign, "ABC123");
    }

    #[test]
    fn default_radius_stays_within_one_credit() {
        // 150 km at mid-latitudes is well under OpenSky's 25 square degree tier.
        for lat in [0.0, 37.77, 60.0] {
            let [lamin, lomin, lamax, lomax] = bounding_box(lat, 0.0, 150.0);
            let area = (lamax - lamin) * (lomax - lomin);
            assert!(area <= 25.0, "{lat}: {area}");
        }
    }
}
