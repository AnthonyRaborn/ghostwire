//! Last good reading per source, so a restart shows ghost data straight away. Each
//! entry records the config its reading was taken under, and is skipped once that
//! config changes or the entry grows too old to be worth showing.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::reading::Reading;
use crate::source::SourceId;

/// Older ghosts are history, not a head start.
const MAX_AGE: TimeDelta = TimeDelta::hours(24);

pub struct Cache {
    dir: PathBuf,
    config: Config,
}

#[derive(Serialize)]
struct EntryRef<'a> {
    fetched_at: DateTime<Utc>,
    config: String,
    reading: &'a Reading,
}

#[derive(Deserialize)]
struct Entry {
    fetched_at: DateTime<Utc>,
    /// Missing from entries written before it existed, which then only match sources
    /// that don't depend on config.
    #[serde(default)]
    config: String,
    reading: Reading,
}

impl Cache {
    pub fn open(dir: &Path, config: &Config) -> Result<Self> {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        Ok(Self {
            dir: dir.to_path_buf(),
            config: config.clone(),
        })
    }

    fn path(&self, source: SourceId) -> PathBuf {
        self.dir.join(format!("{source:?}.json").to_lowercase())
    }

    /// A missing, unreadable, outdated, or too-old entry is just a cold start.
    pub fn load(&self, source: SourceId) -> Option<(DateTime<Utc>, Reading)> {
        let text = std::fs::read_to_string(self.path(source)).ok()?;
        let entry = match serde_json::from_str::<Entry>(&text) {
            Ok(entry) => entry,
            Err(e) => {
                tracing::warn!(?source, "ignoring unreadable cache entry: {e}");
                return None;
            }
        };
        if entry.config != fingerprint(&self.config, source) {
            tracing::info!(?source, "ignoring cache entry from a different config");
            return None;
        }
        if Utc::now() - entry.fetched_at > MAX_AGE {
            tracing::info!(?source, "ignoring cache entry older than {MAX_AGE}");
            return None;
        }
        Some((entry.fetched_at, entry.reading))
    }

    /// Writes via a temp file and rename, so a crash mid-write can't leave a torn entry.
    pub fn store(&self, source: SourceId, fetched_at: DateTime<Utc>, reading: &Reading) {
        let path = self.path(source);
        let tmp = path.with_extension("json.tmp");
        let result = serde_json::to_vec(&EntryRef {
            fetched_at,
            config: fingerprint(&self.config, source),
            reading,
        })
        .map_err(anyhow::Error::from)
        .and_then(|bytes| Ok(std::fs::write(&tmp, bytes)?))
        .and_then(|()| Ok(std::fs::rename(&tmp, &path)?));
        if let Err(e) = result {
            tracing::warn!(?source, "cache write failed: {e}");
        }
    }
}

/// The config a source's reading depends on, so a ghost from another sector, ticker
/// list, or feed list isn't shown as if it were this rig's.
fn fingerprint(config: &Config, source: SourceId) -> String {
    let sector = &config.sector;
    match source {
        SourceId::Stocks => format!("{:?}", config.zaibatsu.stocks),
        SourceId::Crypto => format!("{:?}", config.zaibatsu.coins),
        SourceId::Weather => format!("{:?} {:?}", sector.fix(), sector.units),
        SourceId::Quakes => format!("{:?} {}", sector.fix(), sector.radius_km),
        SourceId::OpenSky => format!("{:?} {}", sector.fix(), sector.flight_radius_km),
        SourceId::Orbit => format!("{:?}", sector.fix()),
        SourceId::Rss => format!("{:?}", config.intercepts.rss),
        SourceId::Ioda => format!("{:?}", sector.country),
        SourceId::Hn | SourceId::Kev | SourceId::Swpc | SourceId::Uplink => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reading::Quote;

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ghostwire-cache-{name}-{}", std::process::id()))
    }

    fn crypto() -> Reading {
        Reading::Crypto(vec![Quote {
            symbol: "BTC".into(),
            price: 1.0,
            change_pct: 0.5,
            spark: vec![1.0, 2.0],
        }])
    }

    #[test]
    fn round_trips_a_reading() {
        let dir = temp_dir("round-trip");
        let cache = Cache::open(&dir, &Config::default()).unwrap();
        assert!(cache.load(SourceId::Crypto).is_none());

        let at = Utc::now();
        let reading = crypto();
        cache.store(SourceId::Crypto, at, &reading);
        let (loaded_at, loaded) = cache.load(SourceId::Crypto).unwrap();
        assert_eq!(loaded_at, at);
        assert!(matches!(loaded, Reading::Crypto(q) if q[0].symbol == "BTC"));

        std::fs::write(cache.path(SourceId::Crypto), "not json").unwrap();
        assert!(cache.load(SourceId::Crypto).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn skips_entries_from_another_config() {
        let dir = temp_dir("config");
        let before = Config::parse("[sector]\nlat = 37.77\nlon = -122.42").unwrap();
        let cache = Cache::open(&dir, &before).unwrap();
        let hn = Reading::Hn(Vec::new());
        cache.store(SourceId::Crypto, Utc::now(), &crypto());
        cache.store(SourceId::Quakes, Utc::now(), &Reading::Quakes(Vec::new()));
        cache.store(SourceId::Hn, Utc::now(), &hn);

        // Moving the sector orphans the quakes, but not crypto or HN.
        let moved = Config::parse("[sector]\nlat = 40.71\nlon = -74.01").unwrap();
        let cache = Cache::open(&dir, &moved).unwrap();
        assert!(cache.load(SourceId::Quakes).is_none());
        assert!(cache.load(SourceId::Crypto).is_some());
        assert!(cache.load(SourceId::Hn).is_some());

        let recoined = Config::parse("[zaibatsu]\ncoins = [\"monero\"]").unwrap();
        let cache = Cache::open(&dir, &recoined).unwrap();
        assert!(cache.load(SourceId::Crypto).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn skips_entries_past_max_age_and_reads_old_formats() {
        let dir = temp_dir("age");
        let cache = Cache::open(&dir, &Config::default()).unwrap();
        let stale = Utc::now() - MAX_AGE - TimeDelta::minutes(1);
        cache.store(SourceId::Crypto, stale, &crypto());
        assert!(cache.load(SourceId::Crypto).is_none());

        // An entry from before fingerprints still loads for a config-free source.
        let hn = serde_json::to_value(Reading::Hn(Vec::new())).unwrap();
        let old = serde_json::json!({ "fetched_at": Utc::now(), "reading": hn });
        std::fs::write(cache.path(SourceId::Hn), old.to_string()).unwrap();
        assert!(cache.load(SourceId::Hn).is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
