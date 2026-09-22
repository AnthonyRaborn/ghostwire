//! SEISMIC: USGS earthquakes. With a sector fix, it merges every quake within the
//! radius over the past week with M4.5+ worldwide over the past day. Without one, it
//! shows only the worldwide feed. No key needed.

use std::collections::HashSet;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::http::{get_text, parse_json};
use super::{Feed, FetchError};
use crate::config::Config;
use crate::geo;
use crate::reading::{Quake, Reading};
use crate::source::SourceId;

const GLOBAL_URL: &str =
    "https://earthquake.usgs.gov/earthquakes/feed/v1.0/summary/4.5_day.geojson";
const NEARBY_DAYS: i64 = 7;
const NEARBY_LIMIT: usize = 15;
const MAX_QUAKES: usize = 20;

pub struct Usgs {
    fix: Option<(f64, f64)>,
    radius_km: f64,
}

impl Usgs {
    pub fn new(config: &Config) -> Self {
        Self {
            fix: config.sector.fix(),
            radius_km: config.sector.radius_km,
        }
    }
}

impl Feed for Usgs {
    fn source(&self) -> SourceId {
        SourceId::Quakes
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(2 * 60)
    }

    async fn fetch(&self, http: &reqwest::Client) -> Result<Reading, FetchError> {
        let global = get_text(http.get(GLOBAL_URL));
        let Some((lat, lon)) = self.fix else {
            let quakes = parse(&global.await?, None).map_err(FetchError::Failed)?;
            return Ok(Reading::Quakes(quakes));
        };
        let since = (Utc::now() - chrono::Duration::days(NEARBY_DAYS)).format("%Y-%m-%dT%H:%M");
        let nearby_url = format!(
            "https://earthquake.usgs.gov/fdsnws/event/1/query?format=geojson\
             &latitude={lat:.4}&longitude={lon:.4}&maxradiuskm={:.0}\
             &starttime={since}&orderby=time&limit={NEARBY_LIMIT}&eventtype=earthquake",
            self.radius_km
        );
        let (global, nearby) = tokio::join!(global, get_text(http.get(nearby_url)));
        let global = parse(&global?, self.fix).map_err(FetchError::Failed)?;
        let nearby = parse(&nearby?, self.fix).map_err(FetchError::Failed)?;
        Ok(Reading::Quakes(merge(nearby, global)))
    }
}

#[derive(Deserialize)]
struct Collection {
    features: Vec<Feature>,
}

#[derive(Deserialize)]
struct Feature {
    id: String,
    properties: Properties,
    geometry: Geometry,
}

#[derive(Deserialize)]
struct Properties {
    mag: Option<f64>,
    place: Option<String>,
    /// Milliseconds since the Unix epoch.
    time: i64,
    tsunami: Option<u8>,
    #[serde(rename = "type")]
    kind: Option<String>,
}

#[derive(Deserialize)]
struct Geometry {
    /// `[lon, lat, depth_km]`
    coordinates: Vec<f64>,
}

/// Parses a USGS GeoJSON feed, keeping earthquakes with a magnitude and location.
pub fn parse(text: &str, fix: Option<(f64, f64)>) -> Result<Vec<Quake>, String> {
    let collection: Collection = parse_json(text)?;
    Ok(collection
        .features
        .into_iter()
        .filter(|f| {
            f.properties
                .kind
                .as_deref()
                .is_none_or(|k| k == "earthquake")
        })
        .filter_map(|f| {
            let &[lon, lat, depth, ..] = f.geometry.coordinates.as_slice() else {
                return None;
            };
            let (distance_km, bearing) = match fix {
                Some((here_lat, here_lon)) => (
                    Some(geo::distance_km(here_lat, here_lon, lat, lon)),
                    Some(geo::bearing_deg(here_lat, here_lon, lat, lon)),
                ),
                None => (None, None),
            };
            Some(Quake {
                mag: f.properties.mag?,
                place: f
                    .properties
                    .place
                    .unwrap_or_else(|| "unknown location".into()),
                time: DateTime::from_timestamp_millis(f.properties.time)?,
                depth_km: depth,
                distance_km,
                bearing,
                tsunami: f.properties.tsunami.unwrap_or(0) != 0,
                id: f.id,
            })
        })
        .collect())
}

/// Union by event id, newest first.
fn merge(nearby: Vec<Quake>, global: Vec<Quake>) -> Vec<Quake> {
    let mut seen = HashSet::new();
    let mut quakes: Vec<Quake> = nearby
        .into_iter()
        .chain(global)
        .filter(|q| seen.insert(q.id.clone()))
        .collect();
    quakes.sort_by_key(|q| std::cmp::Reverse(q.time));
    quakes.truncate(MAX_QUAKES);
    quakes
}

#[cfg(test)]
mod tests {
    use super::*;

    const GLOBAL: &str = include_str!("../../tests/fixtures/usgs_global.json");
    const NEARBY: &str = include_str!("../../tests/fixtures/usgs_near.json");
    const SF: (f64, f64) = (37.77, -122.42);

    #[test]
    fn parses_global_feed_without_a_fix() {
        let quakes = parse(GLOBAL, None).unwrap();
        assert_eq!(quakes.len(), 11);
        let q = &quakes[0];
        assert_eq!(q.id, "us7000tj1s");
        assert_eq!(q.mag, 5.2);
        assert_eq!(q.place, "92 km S of Sarangani, Philippines");
        assert_eq!(q.depth_km, 10.0);
        assert!(q.distance_km.is_none());
    }

    #[test]
    fn nearby_quakes_get_distance_and_bearing() {
        let quakes = parse(NEARBY, Some(SF)).unwrap();
        assert_eq!(quakes.len(), 15);
        // "4 km N of Santa Rosa, CA" is roughly 83 km NNW of San Francisco.
        let q = &quakes[0];
        let d = q.distance_km.unwrap();
        assert!((75.0..95.0).contains(&d), "{d}");
        assert_eq!(geo::compass(q.bearing.unwrap()), "N");
        assert!(quakes.iter().all(|q| q.distance_km.unwrap() <= 300.0));
    }

    #[test]
    fn merge_dedupes_and_sorts_newest_first() {
        let nearby = parse(NEARBY, Some(SF)).unwrap();
        let global = parse(GLOBAL, Some(SF)).unwrap();
        let mut doubled = nearby.clone();
        doubled.extend(nearby.iter().cloned());
        let merged = merge(doubled, global);
        let ids: HashSet<_> = merged.iter().map(|q| &q.id).collect();
        assert_eq!(ids.len(), merged.len());
        assert!(merged.windows(2).all(|w| w[0].time >= w[1].time));
        assert!(merged.len() <= MAX_QUAKES);
    }

    #[test]
    fn skips_non_earthquakes_and_missing_magnitudes() {
        let text = r#"{"features":[
            {"id":"a","properties":{"mag":1.2,"place":"x","time":0,"type":"quarry blast"},"geometry":{"coordinates":[0,0,0]}},
            {"id":"b","properties":{"mag":null,"place":"x","time":0,"type":"earthquake"},"geometry":{"coordinates":[0,0,0]}},
            {"id":"c","properties":{"mag":2.0,"place":null,"time":0},"geometry":{"coordinates":[0,0,5]}}
        ]}"#;
        let quakes = parse(text, None).unwrap();
        assert_eq!(quakes.len(), 1);
        assert_eq!(quakes[0].id, "c");
        assert_eq!(quakes[0].place, "unknown location");
    }
}
