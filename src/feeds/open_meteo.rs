//! ATMOS: current conditions and the next 24h from Open-Meteo, plus air quality and UV
//! from its separate air-quality API. No key needed.

use std::time::Duration;

use serde::Deserialize;

use super::http::{get_text, parse_json};
use super::{Feed, FetchError};
use crate::config::{Config, Units};
use crate::reading::{Reading, Weather};
use crate::source::SourceId;

pub struct OpenMeteo {
    fix: Option<(f64, f64)>,
    units: Units,
}

impl OpenMeteo {
    pub fn new(config: &Config) -> Self {
        Self {
            fix: config.sector.fix(),
            units: config.sector.units,
        }
    }
}

impl Feed for OpenMeteo {
    fn source(&self) -> SourceId {
        SourceId::Weather
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(10 * 60)
    }

    async fn fetch(&self, http: &reqwest::Client) -> Result<Reading, FetchError> {
        let Some((lat, lon)) = self.fix else {
            return Err(FetchError::NotConfigured("set [sector] lat/lon".into()));
        };
        let units = match self.units {
            Units::Metric => "",
            Units::Imperial => "&temperature_unit=fahrenheit&wind_speed_unit=mph",
        };
        let forecast_url = format!(
            "https://api.open-meteo.com/v1/forecast?latitude={lat:.4}&longitude={lon:.4}\
             &current=temperature_2m,apparent_temperature,relative_humidity_2m,\
             wind_speed_10m,wind_direction_10m,weather_code,is_day\
             &hourly=temperature_2m,precipitation_probability&forecast_hours=24{units}"
        );
        let air_url = format!(
            "https://air-quality-api.open-meteo.com/v1/air-quality\
             ?latitude={lat:.4}&longitude={lon:.4}&current=us_aqi,uv_index"
        );
        let (forecast, air) = tokio::join!(
            get_text(http.get(forecast_url)),
            get_text(http.get(air_url))
        );
        let forecast = forecast?;
        // Air quality is a nice-to-have; losing it shouldn't take the node down.
        let air = air
            .inspect_err(|e| tracing::warn!("air quality unavailable: {e:?}"))
            .ok();
        parse(&forecast, air.as_deref(), self.units)
            .map(Reading::Weather)
            .map_err(FetchError::Failed)
    }
}

#[derive(Deserialize)]
struct Forecast {
    current: Current,
    hourly: Hourly,
}

#[derive(Deserialize)]
struct Current {
    temperature_2m: f64,
    apparent_temperature: f64,
    relative_humidity_2m: f64,
    wind_speed_10m: f64,
    wind_direction_10m: f64,
    weather_code: u8,
    is_day: u8,
}

#[derive(Deserialize)]
struct Hourly {
    temperature_2m: Vec<Option<f64>>,
    precipitation_probability: Vec<Option<f64>>,
}

#[derive(Deserialize)]
struct Air {
    current: AirCurrent,
}

#[derive(Deserialize)]
struct AirCurrent {
    us_aqi: Option<f64>,
    uv_index: Option<f64>,
}

pub fn parse(forecast: &str, air: Option<&str>, units: Units) -> Result<Weather, String> {
    let Forecast { current: c, hourly } = parse_json(forecast)?;
    let air = air.and_then(|text| {
        parse_json::<Air>(text)
            .inspect_err(|e| tracing::warn!("air quality: {e}"))
            .ok()
            .map(|a| a.current)
    });
    Ok(Weather {
        units,
        temp: c.temperature_2m,
        feels_like: c.apparent_temperature,
        humidity: c.relative_humidity_2m,
        // The hourly series starts at the current hour.
        precip_prob: hourly
            .precipitation_probability
            .first()
            .copied()
            .flatten()
            .unwrap_or(0.0),
        wind_speed: c.wind_speed_10m,
        wind_from: c.wind_direction_10m,
        code: c.weather_code,
        is_day: c.is_day != 0,
        us_aqi: air.as_ref().and_then(|a| a.us_aqi),
        uv_index: air.as_ref().and_then(|a| a.uv_index),
        next_24h: hourly.temperature_2m.into_iter().flatten().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORECAST: &str = include_str!("../../tests/fixtures/open_meteo_forecast.json");
    const AIR: &str = include_str!("../../tests/fixtures/open_meteo_air.json");

    #[test]
    fn parses_forecast_and_air() {
        let w = parse(FORECAST, Some(AIR), Units::Metric).unwrap();
        assert_eq!(w.temp, 14.4);
        assert_eq!(w.feels_like, 12.9);
        assert_eq!(w.humidity, 90.0);
        assert_eq!(w.wind_from, 264.0);
        assert_eq!(w.code, 1);
        assert!(!w.is_day);
        assert_eq!(w.next_24h.len(), 24);
        assert_eq!(w.us_aqi, Some(51.0));
        assert_eq!(w.uv_index, Some(0.0));
    }

    #[test]
    fn survives_missing_or_broken_air_quality() {
        let w = parse(FORECAST, None, Units::Metric).unwrap();
        assert_eq!(w.us_aqi, None);
        let w = parse(FORECAST, Some("<html>502</html>"), Units::Metric).unwrap();
        assert_eq!(w.uv_index, None);
    }

    #[tokio::test]
    async fn offline_without_a_sector_fix() {
        let feed = OpenMeteo::new(&Config::default());
        let result = feed.fetch(&reqwest::Client::new()).await;
        assert!(matches!(result, Err(FetchError::NotConfigured(_))));
    }

    #[test]
    fn rejects_a_broken_forecast() {
        assert!(parse("{}", Some(AIR), Units::Metric).is_err());
    }
}
