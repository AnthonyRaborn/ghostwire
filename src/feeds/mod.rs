//! Each source runs as its own tokio task on its own interval, reporting to the app
//! loop over the shared message channel. The UI never waits on the network.

pub mod coingecko;
pub mod demo;
pub mod finnhub;
pub mod hn;
mod http;
pub mod kev;
pub mod open_meteo;
pub mod opensky;
pub mod orbit;
pub mod rss;
pub mod swpc;
pub mod usgs;

use std::future::Future;
use std::time::Duration;

use tokio::sync::broadcast::{self, error::RecvError};
use tokio::sync::mpsc::UnboundedSender;

use crate::cache::Cache;
use crate::config::Config;
use crate::event::Msg;
use crate::keys::Keys;
use crate::reading::Reading;
use crate::source::SourceId;

const FETCH_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_BACKOFF: Duration = Duration::from_secs(15 * 60);

#[derive(Debug)]
pub enum FetchError {
    /// Network error, bad status, or unparseable body. Shown as ICE.
    Failed(String),
    /// Upstream said slow down (HTTP 429), with its Retry-After if given. Shown as TRACE.
    RateLimited(Option<Duration>),
    /// Missing key or location; retrying won't help until config changes. Shown as OFFLINE.
    NotConfigured(String),
}

pub trait Feed: Send + Sync + 'static {
    fn source(&self) -> SourceId;

    /// How long to wait after a successful fetch.
    fn interval(&self) -> Duration;

    /// First retry delay after a failure; doubles with each consecutive failure.
    fn retry_base(&self) -> Duration {
        Duration::from_secs(15)
    }

    fn fetch(
        &self,
        http: &reqwest::Client,
    ) -> impl Future<Output = Result<Reading, FetchError>> + Send;
}

#[derive(Debug)]
pub enum FeedMsg {
    /// A fetch just started.
    Attempt(SourceId),
    Done {
        source: SourceId,
        result: Result<Reading, FetchError>,
        /// Consecutive failures, including this one.
        failures: u32,
        interval: Duration,
        /// When the next attempt is scheduled; `None` means not until a re-breach.
        retry_in: Option<Duration>,
    },
}

/// Starts every enabled source and returns what was started, with each one's
/// refresh interval.
pub fn spawn_all(
    config: &Config,
    keys: &Keys,
    demo: bool,
    http: &reqwest::Client,
    tx: &UnboundedSender<Msg>,
    rebreach: &broadcast::Sender<()>,
    cache: Option<&Cache>,
) -> Vec<(SourceId, Duration)> {
    let mut launcher = Launcher {
        http,
        tx,
        rebreach,
        started: Vec::new(),
    };
    for source in SourceId::ALL {
        if !enabled(config, source) {
            continue;
        }
        if demo {
            launcher.launch(demo::DemoFeed::new(source, config));
            continue;
        }
        match source {
            SourceId::Stocks => {
                let cached = cache.and_then(|c| c.load(SourceId::Stocks)).map(|(_, r)| r);
                launcher.launch(finnhub::Finnhub::new(
                    config,
                    keys.finnhub(),
                    cached.as_ref(),
                ));
            }
            SourceId::Crypto => {
                launcher.launch(coingecko::CoinGecko::new(config, keys.coingecko()));
            }
            SourceId::Weather => launcher.launch(open_meteo::OpenMeteo::new(config)),
            SourceId::Hn => launcher.launch(hn::HackerNews),
            SourceId::Kev => launcher.launch(kev::Kev),
            SourceId::Quakes => launcher.launch(usgs::Usgs::new(config)),
            SourceId::Swpc => launcher.launch(swpc::Swpc),
            SourceId::OpenSky => launcher.launch(opensky::OpenSky::new(config)),
            SourceId::Orbit => launcher.launch(orbit::Orbit::new(config)),
            SourceId::Rss => launcher.launch(rss::Rss::new(config)),
        }
    }
    launcher.started
}

struct Launcher<'a> {
    http: &'a reqwest::Client,
    tx: &'a UnboundedSender<Msg>,
    rebreach: &'a broadcast::Sender<()>,
    started: Vec<(SourceId, Duration)>,
}

impl Launcher<'_> {
    fn launch<F: Feed>(&mut self, feed: F) {
        self.started.push((feed.source(), feed.interval()));
        spawn(
            feed,
            self.http.clone(),
            self.tx.clone(),
            self.rebreach.subscribe(),
        );
    }
}

/// Sources the config explicitly turns off by listing nothing for them.
fn enabled(config: &Config, source: SourceId) -> bool {
    match source {
        SourceId::Stocks => !config.zaibatsu.stocks.is_empty(),
        SourceId::Crypto => !config.zaibatsu.coins.is_empty(),
        SourceId::Rss => !config.intercepts.rss.is_empty(),
        _ => true,
    }
}

pub fn spawn<F: Feed>(
    feed: F,
    http: reqwest::Client,
    tx: UnboundedSender<Msg>,
    mut rebreach: broadcast::Receiver<()>,
) {
    tokio::spawn(async move {
        let source = feed.source();
        let mut failures = 0u32;
        loop {
            if tx.send(Msg::Feed(FeedMsg::Attempt(source))).is_err() {
                return;
            }
            let result = match tokio::time::timeout(FETCH_TIMEOUT, feed.fetch(&http)).await {
                Ok(result) => result,
                Err(_) => Err(FetchError::Failed(format!(
                    "no response in {}s",
                    FETCH_TIMEOUT.as_secs()
                ))),
            };
            let retry_in = match &result {
                Ok(_) => {
                    failures = 0;
                    Some(feed.interval())
                }
                Err(FetchError::Failed(e)) => {
                    failures += 1;
                    tracing::warn!(?source, failures, "fetch failed: {e}");
                    Some(backoff(feed.retry_base(), failures))
                }
                Err(FetchError::RateLimited(after)) => {
                    failures += 1;
                    tracing::warn!(?source, ?after, "rate limited");
                    Some(after.unwrap_or_else(|| backoff(feed.retry_base() * 4, failures)))
                }
                Err(FetchError::NotConfigured(reason)) => {
                    tracing::info!(?source, "offline: {reason}");
                    None
                }
            };
            let done = FeedMsg::Done {
                source,
                result,
                failures,
                interval: feed.interval(),
                retry_in,
            };
            if tx.send(Msg::Feed(done)).is_err() {
                return;
            }
            let woken = match retry_in {
                Some(wait) => tokio::select! {
                    _ = tokio::time::sleep(wait) => true,
                    r = rebreach.recv() => !matches!(r, Err(RecvError::Closed)),
                },
                None => !matches!(rebreach.recv().await, Err(RecvError::Closed)),
            };
            if !woken {
                return;
            }
        }
    });
}

/// Exponential backoff with ±20% jitter, so sources that fail together don't retry
/// in lockstep.
fn backoff(base: Duration, failures: u32) -> Duration {
    let doublings = failures.saturating_sub(1).min(16);
    let wait = base.saturating_mul(1 << doublings).min(MAX_BACKOFF);
    wait.mul_f64(0.8 + fastrand::f64() * 0.4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_and_caps() {
        let base = Duration::from_secs(10);
        let first = backoff(base, 1);
        assert!(first >= Duration::from_secs(8) && first <= Duration::from_secs(12));
        let third = backoff(base, 3);
        assert!(third >= Duration::from_secs(32) && third <= Duration::from_secs(48));
        assert!(backoff(base, 40) <= MAX_BACKOFF.mul_f64(1.2));
    }
}
