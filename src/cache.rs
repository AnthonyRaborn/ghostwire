//! Last good reading per source, so a restart shows ghost data straight away.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::reading::Reading;
use crate::source::SourceId;

pub struct Cache {
    dir: PathBuf,
}

#[derive(Serialize)]
struct EntryRef<'a> {
    fetched_at: DateTime<Utc>,
    reading: &'a Reading,
}

#[derive(Deserialize)]
struct Entry {
    fetched_at: DateTime<Utc>,
    reading: Reading,
}

impl Cache {
    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        Ok(Self {
            dir: dir.to_path_buf(),
        })
    }

    fn path(&self, source: SourceId) -> PathBuf {
        self.dir.join(format!("{source:?}.json").to_lowercase())
    }

    /// A missing or unreadable entry is just a cold start.
    pub fn load(&self, source: SourceId) -> Option<(DateTime<Utc>, Reading)> {
        let text = std::fs::read_to_string(self.path(source)).ok()?;
        match serde_json::from_str::<Entry>(&text) {
            Ok(entry) => Some((entry.fetched_at, entry.reading)),
            Err(e) => {
                tracing::warn!(?source, "ignoring unreadable cache entry: {e}");
                None
            }
        }
    }

    /// Writes via a temp file and rename, so a crash mid-write can't leave a torn entry.
    pub fn store(&self, source: SourceId, fetched_at: DateTime<Utc>, reading: &Reading) {
        let path = self.path(source);
        let tmp = path.with_extension("json.tmp");
        let result = serde_json::to_vec(&EntryRef {
            fetched_at,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reading::Quote;

    #[test]
    fn round_trips_a_reading() {
        let dir = std::env::temp_dir().join(format!("ghostwire-cache-test-{}", std::process::id()));
        let cache = Cache::open(&dir).unwrap();
        assert!(cache.load(SourceId::Crypto).is_none());

        let at = Utc::now();
        let reading = Reading::Crypto(vec![Quote {
            symbol: "BTC".into(),
            price: 1.0,
            change_pct: 0.5,
            spark: vec![1.0, 2.0],
        }]);
        cache.store(SourceId::Crypto, at, &reading);
        let (loaded_at, loaded) = cache.load(SourceId::Crypto).unwrap();
        assert_eq!(loaded_at, at);
        assert!(matches!(loaded, Reading::Crypto(q) if q[0].symbol == "BTC"));

        std::fs::write(cache.path(SourceId::Crypto), "not json").unwrap();
        assert!(cache.load(SourceId::Crypto).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
