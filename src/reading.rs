//! What each source delivers. These are also what the disk cache stores.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::config::Units;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Reading {
    Stocks(Vec<Quote>),
    Crypto(Vec<Quote>),
    Weather(Weather),
    Hn(Vec<Story>),
    Kev(Vec<Vuln>),
    Quakes(Vec<Quake>),
    Swpc(SpaceWeather),
    OpenSky(Vec<Contact>),
    Orbit(Vec<Satellite>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quote {
    /// Display symbol, e.g. `NVDA` or `BTC`.
    pub symbol: String,
    pub price: f64,
    /// Percent change over the session (stocks) or 24h (crypto).
    pub change_pct: f64,
    /// Recent prices, oldest first.
    pub spark: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Weather {
    pub units: Units,
    pub temp: f64,
    pub feels_like: f64,
    pub humidity: f64,
    pub precip_prob: f64,
    pub wind_speed: f64,
    /// Direction the wind blows *from*, in degrees.
    pub wind_from: f64,
    /// WMO weather interpretation code.
    pub code: u8,
    pub is_day: bool,
    pub us_aqi: Option<f64>,
    pub uv_index: Option<f64>,
    /// Hourly temperature for the next 24h.
    pub next_24h: Vec<f64>,
    /// Precipitation probability and amount for each of the next few hours.
    pub precip_next: Vec<PrecipHour>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PrecipHour {
    pub prob: f64,
    /// Millimeters, whatever the display units — Open-Meteo always reports metric here.
    pub mm: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Story {
    pub id: u64,
    pub title: String,
    pub score: u32,
    pub comments: u32,
    pub posted: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vuln {
    pub cve: String,
    pub vendor: String,
    pub product: String,
    /// CISA's name for it, e.g. "Zyxel GS1900 Series Switches Stack-Based Buffer Overflow".
    #[serde(default)]
    pub name: String,
    pub added: NaiveDate,
    pub ransomware: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quake {
    pub id: String,
    pub mag: f64,
    pub place: String,
    pub time: DateTime<Utc>,
    pub depth_km: f64,
    /// Distance and bearing from the sector, when a location is configured.
    pub distance_km: Option<f64>,
    pub bearing: Option<f64>,
    pub tsunami: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceWeather {
    pub kp: f64,
    /// Recent Kp readings, oldest first.
    pub kp_history: Vec<f64>,
    /// GOES long-channel X-ray flux, W/m².
    pub xray_flux: Option<f64>,
    pub scales: Scales,
}

/// NOAA space weather scales: geomagnetic storms, solar radiation, radio blackouts.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Scales {
    pub g: u8,
    pub s: u8,
    pub r: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    pub callsign: String,
    pub altitude_m: Option<f64>,
    pub speed_ms: Option<f64>,
    pub heading: Option<f64>,
    pub distance_km: f64,
    pub bearing: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Satellite {
    pub name: String,
    pub altitude_km: f64,
    pub velocity_kmh: f64,
    /// Sunlit vs. in Earth's shadow — the upstream API's own read, not derived.
    pub sunlit: bool,
    /// Ground-track distance and bearing from the sector.
    pub distance_km: f64,
    pub bearing: f64,
    /// Above the sector's horizon when positive.
    pub elevation_deg: f64,
}

/// Solar flare class for a GOES X-ray flux, e.g. `2.3e-6` → `"C2.3"`.
pub fn xray_class(flux: f64) -> String {
    let (letter, base) = match flux {
        f if f >= 1e-4 => ('X', 1e-4),
        f if f >= 1e-5 => ('M', 1e-5),
        f if f >= 1e-6 => ('C', 1e-6),
        f if f >= 1e-7 => ('B', 1e-7),
        _ => ('A', 1e-8),
    };
    format!("{letter}{:.1}", flux / base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_flares() {
        assert_eq!(xray_class(2.3e-6), "C2.3");
        assert_eq!(xray_class(1.0e-5), "M1.0");
        assert_eq!(xray_class(4.5e-4), "X4.5");
        assert_eq!(xray_class(5.0e-8), "A5.0");
    }
}
