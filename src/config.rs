use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Deserializer, Serialize};

pub const EXAMPLE: &str = include_str!("../config.example.toml");

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub sector: Sector,
    pub zaibatsu: Zaibatsu,
    pub intercepts: Intercepts,
    pub fx: Fx,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sector {
    pub name: String,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub radius_km: f64,
    pub units: Units,
}

impl Default for Sector {
    fn default() -> Self {
        Self {
            name: "SECTOR-0".into(),
            lat: None,
            lon: None,
            radius_km: 300.0,
            units: Units::Metric,
        }
    }
}

impl Sector {
    /// `(lat, lon)` once both are set.
    pub fn fix(&self) -> Option<(f64, f64)> {
        Some((self.lat?, self.lon?))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Units {
    #[default]
    Metric,
    Imperial,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Zaibatsu {
    pub stocks: Vec<String>,
    pub coins: Vec<String>,
}

impl Default for Zaibatsu {
    fn default() -> Self {
        Self {
            stocks: vec!["NVDA".into(), "TSM".into(), "MSFT".into()],
            coins: vec!["bitcoin".into(), "ethereum".into(), "solana".into()],
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Intercepts {
    pub rss: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Fx {
    pub level: FxLevel,
    #[serde(deserialize_with = "de_duration")]
    pub breach_every: Duration,
    #[serde(deserialize_with = "de_duration")]
    pub breach_hold: Duration,
}

impl Default for Fx {
    fn default() -> Self {
        Self {
            level: FxLevel::Active,
            breach_every: Duration::from_secs(45),
            breach_hold: Duration::from_secs(15),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum FxLevel {
    Calm,
    #[default]
    Active,
    Chaotic,
}

impl Config {
    /// Loads `path`, or returns defaults if it doesn't exist. The bool says whether a
    /// file was found.
    pub fn load(path: &Path) -> Result<(Self, bool)> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok((Self::default(), false));
            }
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        let config = Self::parse(&text).with_context(|| format!("in {}", path.display()))?;
        Ok((config, true))
    }

    pub fn parse(text: &str) -> Result<Self> {
        let config: Self = toml::from_str(text)?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        let s = &self.sector;
        if s.lat.is_some() != s.lon.is_some() {
            bail!("[sector] needs both lat and lon, or neither");
        }
        if let Some(lat) = s.lat
            && !(-90.0..=90.0).contains(&lat)
        {
            bail!("[sector] lat {lat} is outside -90..90");
        }
        if let Some(lon) = s.lon
            && !(-180.0..=180.0).contains(&lon)
        {
            bail!("[sector] lon {lon} is outside -180..180");
        }
        if s.radius_km <= 0.0 {
            bail!("[sector] radius_km must be positive");
        }
        if self.fx.breach_hold >= self.fx.breach_every {
            bail!("[fx] breach_hold must be shorter than breach_every");
        }
        Ok(())
    }
}

fn de_duration<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
    let s = String::deserialize(d)?;
    parse_duration(&s).map_err(serde::de::Error::custom)
}

/// Parses `"500ms"`, `"45s"`, `"2m"`, or `"1h"`.
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let n: u64 = num
        .parse()
        .map_err(|_| format!("bad duration {s:?}: expected e.g. \"45s\""))?;
    match unit.trim() {
        "ms" => Ok(Duration::from_millis(n)),
        "s" => Ok(Duration::from_secs(n)),
        "m" => Ok(Duration::from_secs(n * 60)),
        "h" => Ok(Duration::from_secs(n * 3600)),
        _ => Err(format!("bad duration {s:?}: unit must be ms, s, m, or h")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_config_parses() {
        let config = Config::parse(EXAMPLE).unwrap();
        assert_eq!(config.sector.name, "SECTOR-4");
        assert_eq!(config.fx.breach_every, Duration::from_secs(45));
        assert!(config.sector.fix().is_none());
    }

    #[test]
    fn empty_config_is_all_defaults() {
        let config = Config::parse("").unwrap();
        assert_eq!(config.sector.radius_km, 300.0);
        assert_eq!(config.fx.level, FxLevel::Active);
    }

    #[test]
    fn rejects_half_a_location() {
        assert!(Config::parse("[sector]\nlat = 10.0").is_err());
    }

    #[test]
    fn rejects_out_of_range_coordinates() {
        assert!(Config::parse("[sector]\nlat = 91.0\nlon = 0.0").is_err());
    }

    #[test]
    fn rejects_unknown_keys() {
        assert!(Config::parse("[sector]\nlattitude = 10.0").is_err());
    }

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration("45s"), Ok(Duration::from_secs(45)));
        assert_eq!(parse_duration("2m"), Ok(Duration::from_secs(120)));
        assert_eq!(parse_duration("250ms"), Ok(Duration::from_millis(250)));
        assert!(parse_duration("45").is_err());
        assert!(parse_duration("fast").is_err());
    }
}
