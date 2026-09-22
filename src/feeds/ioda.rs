//! NETSTATUS (country outages): IODA's alert feed for the sector's configured country.
//! Keyless. `sector.country` (an ISO 3166-1 alpha-2 code) is this source's own fix —
//! IODA tracks countries and networks, not coordinates, so lat/lon doesn't apply here.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::http::{get_text, parse_json};
use super::{Feed, FetchError};
use crate::config::Config;
use crate::reading::{CountryOutages, OutageAlert, Reading};
use crate::source::SourceId;

const BASE: &str = "https://api.ioda.inetintel.cc.gatech.edu/v2";
/// How far back each poll looks for alerts.
const WINDOW_HOURS: i64 = 3;

pub struct Ioda {
    country: Option<String>,
}

impl Ioda {
    pub fn new(config: &Config) -> Self {
        Self {
            country: config.sector.country.clone(),
        }
    }
}

impl Feed for Ioda {
    fn source(&self) -> SourceId {
        SourceId::Ioda
    }

    fn interval(&self) -> Duration {
        // IODA's own pipeline runs on a few-minute cadence; no need to poll faster,
        // and it's had rough patches, so this errs gentle.
        Duration::from_secs(10 * 60)
    }

    async fn fetch(&self, http: &reqwest::Client) -> Result<Reading, FetchError> {
        let Some(country) = &self.country else {
            return Err(FetchError::NotConfigured("set [sector] country".into()));
        };
        let until = Utc::now().timestamp();
        let from = until - WINDOW_HOURS * 3600;
        let url = format!(
            "{BASE}/outages/alerts?entityType=country&entityCode={country}&from={from}&until={until}"
        );
        let text = get_text(http.get(url)).await?;
        parse(&text, country)
            .map(Reading::Ioda)
            .map_err(FetchError::Failed)
    }
}

#[derive(Deserialize)]
struct Envelope {
    data: Vec<Alert>,
}

#[derive(Deserialize)]
struct Alert {
    datasource: String,
    level: String,
    time: i64,
}

/// Alerts newest first; entries with an unparseable timestamp are dropped rather than
/// failing the whole fetch.
pub fn parse(text: &str, country: &str) -> Result<CountryOutages, String> {
    let envelope: Envelope = parse_json(text)?;
    let mut alerts: Vec<OutageAlert> = envelope
        .data
        .into_iter()
        .filter_map(|a| {
            Some(OutageAlert {
                datasource: a.datasource,
                level: a.level,
                time: DateTime::from_timestamp(a.time, 0)?,
            })
        })
        .collect();
    alerts.sort_by_key(|a| std::cmp::Reverse(a.time));
    Ok(CountryOutages {
        country: country.to_string(),
        alerts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY: &str = include_str!("../../tests/fixtures/ioda_alerts_empty.json");
    const ALERTS: &str = include_str!("../../tests/fixtures/ioda_alerts.json");

    #[test]
    fn parses_a_quiet_window() {
        let outages = parse(EMPTY, "US").unwrap();
        assert_eq!(outages.country, "US");
        assert!(outages.alerts.is_empty());
    }

    #[test]
    fn parses_alerts_newest_first() {
        let outages = parse(ALERTS, "US").unwrap();
        assert_eq!(outages.alerts.len(), 2);
        assert!(outages.alerts[0].time > outages.alerts[1].time);
        assert_eq!(outages.alerts[0].datasource, "bgp");
        assert_eq!(outages.alerts[0].level, "critical");
    }

    #[test]
    fn rejects_a_broken_payload() {
        assert!(parse("{}", "US").is_err());
    }

    #[tokio::test]
    async fn offline_without_a_country() {
        let feed = Ioda::new(&Config::default());
        let result = feed.fetch(&reqwest::Client::new()).await;
        assert!(matches!(result, Err(FetchError::NotConfigured(_))));
    }
}
