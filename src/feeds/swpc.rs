//! HELIOS: space weather from NOAA's Space Weather Prediction Center. The 3-hourly
//! planetary Kp is required; the 1-minute Kp estimate, GOES X-ray flux, and NOAA scales
//! fill in when they're available.

use std::collections::HashMap;
use std::time::Duration;

use serde::Deserialize;

use super::http::{get_text, parse_json};
use super::{Feed, FetchError};
use crate::reading::{Reading, Scales, SpaceWeather};
use crate::source::SourceId;

const KP_URL: &str = "https://services.swpc.noaa.gov/products/noaa-planetary-k-index.json";
const KP_NOW_URL: &str = "https://services.swpc.noaa.gov/json/planetary_k_index_1m.json";
const XRAY_URL: &str = "https://services.swpc.noaa.gov/json/goes/primary/xrays-6-hour.json";
const SCALES_URL: &str = "https://services.swpc.noaa.gov/products/noaa-scales.json";
/// Three days of 3-hour readings.
const KP_HISTORY: usize = 24;
/// The long-wavelength channel that flare classes are defined on.
const XRAY_LONG: &str = "0.1-0.8nm";

pub struct Swpc;

impl Feed for Swpc {
    fn source(&self) -> SourceId {
        SourceId::Swpc
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(10 * 60)
    }

    async fn fetch(&self, http: &reqwest::Client) -> Result<Reading, FetchError> {
        let (kp, kp_now, xray, scales) = tokio::join!(
            get_text(http.get(KP_URL)),
            get_text(http.get(KP_NOW_URL)),
            get_text(http.get(XRAY_URL)),
            get_text(http.get(SCALES_URL)),
        );
        let kp_history = parse_kp(&kp?).map_err(FetchError::Failed)?;
        let optional = |name: &str, text: Result<String, FetchError>| {
            text.inspect_err(|e| tracing::warn!("swpc {name}: {e:?}"))
                .ok()
        };
        let kp_now = optional("kp now", kp_now).and_then(|t| log_err("kp now", parse_kp_now(&t)));
        let xray = optional("xray", xray).and_then(|t| log_err("xray", parse_xray(&t)));
        let scales = optional("scales", scales)
            .and_then(|t| log_err("scales", parse_scales(&t).map(Some)))
            .unwrap_or_default();
        let kp = kp_now.or(kp_history.last().copied()).unwrap_or(0.0);
        Ok(Reading::Swpc(SpaceWeather {
            kp,
            kp_history,
            xray_flux: xray,
            scales,
        }))
    }
}

fn log_err<T>(name: &str, result: Result<Option<T>, String>) -> Option<T> {
    result
        .inspect_err(|e| tracing::warn!("swpc {name}: {e}"))
        .ok()
        .flatten()
}

#[derive(Deserialize)]
struct KpRow {
    #[serde(rename = "Kp")]
    kp: f64,
}

/// The last `KP_HISTORY` 3-hour Kp values, oldest first.
pub fn parse_kp(text: &str) -> Result<Vec<f64>, String> {
    let rows: Vec<KpRow> = parse_json(text)?;
    if rows.is_empty() {
        return Err("empty Kp series".into());
    }
    let start = rows.len().saturating_sub(KP_HISTORY);
    Ok(rows[start..].iter().map(|r| r.kp).collect())
}

#[derive(Deserialize)]
struct KpNowRow {
    estimated_kp: Option<f64>,
}

/// The latest 1-minute estimated Kp.
pub fn parse_kp_now(text: &str) -> Result<Option<f64>, String> {
    let rows: Vec<KpNowRow> = parse_json(text)?;
    Ok(rows.iter().rev().find_map(|r| r.estimated_kp))
}

#[derive(Deserialize)]
struct XrayRow {
    flux: Option<f64>,
    energy: String,
}

/// The latest long-channel X-ray flux, W/m².
pub fn parse_xray(text: &str) -> Result<Option<f64>, String> {
    let rows: Vec<XrayRow> = parse_json(text)?;
    Ok(rows
        .iter()
        .rev()
        .filter(|r| r.energy == XRAY_LONG)
        .find_map(|r| r.flux))
}

#[derive(Deserialize)]
struct ScaleDay {
    #[serde(rename = "G")]
    g: Level,
    #[serde(rename = "S")]
    s: Level,
    #[serde(rename = "R")]
    r: Level,
}

#[derive(Deserialize)]
struct Level {
    #[serde(rename = "Scale")]
    scale: Option<String>,
}

impl Level {
    fn value(&self) -> u8 {
        self.scale
            .as_deref()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0)
    }
}

/// Current G/S/R levels. The payload is keyed by day offset; `"0"` is now.
pub fn parse_scales(text: &str) -> Result<Scales, String> {
    let days: HashMap<String, ScaleDay> = parse_json(text)?;
    let now = days.get("0").ok_or("no current-day scales")?;
    Ok(Scales {
        g: now.g.value(),
        s: now.s.value(),
        r: now.r.value(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const KP: &str = include_str!("../../tests/fixtures/swpc_kp.json");
    const KP_NOW: &str = include_str!("../../tests/fixtures/swpc_kp1m.json");
    const XRAY: &str = include_str!("../../tests/fixtures/swpc_xray.json");
    const SCALES: &str = include_str!("../../tests/fixtures/swpc_scales.json");

    #[test]
    fn parses_kp_series_and_estimate() {
        let history = parse_kp(KP).unwrap();
        assert_eq!(history.len(), KP_HISTORY);
        assert_eq!(*history.last().unwrap(), 1.33);
        assert_eq!(parse_kp_now(KP_NOW).unwrap(), Some(0.67));
    }

    #[test]
    fn picks_the_long_xray_channel() {
        let flux = parse_xray(XRAY).unwrap().unwrap();
        assert!((flux - 4.178_358_210_538_135_6e-7).abs() < 1e-15);
        assert_eq!(crate::reading::xray_class(flux), "B4.2");
    }

    #[test]
    fn parses_scales() {
        let s = parse_scales(SCALES).unwrap();
        assert_eq!((s.g, s.s, s.r), (0, 0, 0));
        let s = parse_scales(r#"{"0":{"G":{"Scale":"2"},"S":{"Scale":null},"R":{"Scale":"1"}}}"#)
            .unwrap();
        assert_eq!((s.g, s.s, s.r), (2, 0, 1));
    }
}
