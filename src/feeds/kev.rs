//! INTERCEPTS (CISA KEV): the newest entries in CISA's Known Exploited Vulnerabilities
//! catalog. The full catalog is ~1.7 MB, so it's polled hourly.

use std::time::Duration;

use chrono::NaiveDate;
use serde::Deserialize;

use super::http::{get_text, parse_json};
use super::{Feed, FetchError};
use crate::reading::{Reading, Vuln};
use crate::source::SourceId;

const URL: &str =
    "https://www.cisa.gov/sites/default/files/feeds/known_exploited_vulnerabilities.json";
const KEEP: usize = 8;

pub struct Kev;

impl Feed for Kev {
    fn source(&self) -> SourceId {
        SourceId::Kev
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(60 * 60)
    }

    async fn fetch(&self, http: &reqwest::Client) -> Result<Reading, FetchError> {
        let text = get_text(http.get(URL)).await?;
        parse(&text).map(Reading::Kev).map_err(FetchError::Failed)
    }
}

#[derive(Deserialize)]
struct Catalog {
    vulnerabilities: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    #[serde(rename = "cveID")]
    cve_id: String,
    vendor_project: String,
    product: String,
    vulnerability_name: Option<String>,
    date_added: NaiveDate,
    known_ransomware_campaign_use: Option<String>,
}

/// The newest `KEEP` entries, newest first.
pub fn parse(text: &str) -> Result<Vec<Vuln>, String> {
    let mut entries = parse_json::<Catalog>(text)?.vulnerabilities;
    // Stable, so same-day entries keep the catalog's own order.
    entries.sort_by_key(|e| std::cmp::Reverse(e.date_added));
    Ok(entries
        .into_iter()
        .take(KEEP)
        .map(|e| Vuln {
            cve: e.cve_id,
            vendor: e.vendor_project,
            product: e.product,
            name: e.vulnerability_name.unwrap_or_default(),
            added: e.date_added,
            ransomware: e.known_ransomware_campaign_use.as_deref() == Some("Known"),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATALOG: &str = include_str!("../../tests/fixtures/cisa_kev.json");

    #[test]
    fn keeps_newest_entries() {
        let vulns = parse(CATALOG).unwrap();
        assert_eq!(vulns.len(), KEEP);
        let first = &vulns[0];
        assert_eq!(first.cve, "CVE-2026-7273");
        assert_eq!(first.vendor, "Zyxel");
        assert!(!first.name.is_empty());
        assert_eq!(first.added, NaiveDate::from_ymd_opt(2026, 9, 21).unwrap());
        assert!(vulns.windows(2).all(|w| w[0].added >= w[1].added));
    }

    #[test]
    fn sorts_by_date_whatever_the_catalog_order() {
        let text = r#"{"vulnerabilities":[
            {"cveID":"CVE-OLD","vendorProject":"a","product":"b","dateAdded":"2021-01-01","knownRansomwareCampaignUse":"Known"},
            {"cveID":"CVE-NEW","vendorProject":"a","product":"b","dateAdded":"2026-01-01","knownRansomwareCampaignUse":"Unknown"}
        ]}"#;
        let vulns = parse(text).unwrap();
        assert_eq!(vulns[0].cve, "CVE-NEW");
        assert!(!vulns[0].ransomware);
        assert!(vulns[1].ransomware);
    }
}
