//! NETSTATUS (own link): times a plain request to Cloudflare's public trace endpoint
//! and reads which edge point-of-presence answered. No key, no coordinates — this
//! measures the rig's own connection, not the sector's.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::http::get_text;
use super::{Feed, FetchError};
use crate::reading::{LinkHealth, Reading};
use crate::source::SourceId;

const URL: &str = "https://www.cloudflare.com/cdn-cgi/trace";
const HISTORY_LEN: usize = 30;

#[derive(Default)]
pub struct Uplink {
    history: Mutex<Vec<f64>>,
}

impl Uplink {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Feed for Uplink {
    fn source(&self) -> SourceId {
        SourceId::Uplink
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(30)
    }

    fn retry_base(&self) -> Duration {
        // A slow or dead uplink is the rig's own problem, worth noticing quickly.
        Duration::from_secs(10)
    }

    async fn fetch(&self, http: &reqwest::Client) -> Result<Reading, FetchError> {
        let started = Instant::now();
        let text = get_text(http.get(URL)).await?;
        let latency_ms = started.elapsed().as_secs_f64() * 1_000.0;
        let colo = parse_colo(&text)
            .ok_or_else(|| FetchError::Failed("no colo in trace response".into()))?;
        let mut history = self.history.lock().unwrap_or_else(|e| e.into_inner());
        history.push(latency_ms);
        if history.len() > HISTORY_LEN {
            history.remove(0);
        }
        Ok(Reading::Uplink(LinkHealth {
            latency_ms,
            colo,
            history: history.clone(),
        }))
    }
}

/// Cloudflare's trace is plain `key=value` lines, not JSON.
fn parse_colo(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.strip_prefix("colo="))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRACE: &str = include_str!("../../tests/fixtures/cf_trace.txt");

    #[test]
    fn parses_the_colo() {
        assert_eq!(parse_colo(TRACE), Some("MIA".to_string()));
    }

    #[test]
    fn missing_colo_is_none() {
        assert_eq!(parse_colo("fl=1\nh=example.com\n"), None);
    }

    #[tokio::test]
    async fn history_accumulates_and_caps() {
        let feed = Uplink::new();
        {
            let mut history = feed.history.lock().unwrap();
            for i in 0..HISTORY_LEN {
                history.push(i as f64);
            }
        }
        // Simulate one more sample landing without going through a real fetch.
        let mut history = feed.history.lock().unwrap();
        history.push(999.0);
        if history.len() > HISTORY_LEN {
            history.remove(0);
        }
        assert_eq!(history.len(), HISTORY_LEN);
        assert_eq!(*history.last().unwrap(), 999.0);
    }
}
