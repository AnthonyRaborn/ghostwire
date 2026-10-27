//! Each source runs as its own tokio task on its own interval, reporting to the app
//! loop over the shared message channel. The UI never waits on the network.

pub mod coingecko;
pub mod demo;
pub mod finnhub;
pub mod hn;
mod http;
pub mod ioda;
pub mod kev;
pub mod open_meteo;
pub mod opensky;
pub mod orbit;
pub mod rss;
pub mod swpc;
pub mod uplink;
pub mod usgs;

use std::future::Future;
use std::time::Duration;

use tokio::sync::broadcast::{
    self,
    error::{RecvError, TryRecvError},
};
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

    /// Why the config keeps this source from running at all (a missing key or
    /// location), as `fetch` would report it without touching the network. Lets
    /// `--check-config` say so up front.
    fn offline(&self) -> Option<String> {
        None
    }

    /// What it can't do as configured, when it still runs.
    fn limited(&self) -> Option<String> {
        None
    }
}

/// Something done with every enabled source's feed, once it's built.
pub trait Visit {
    fn visit<F: Feed>(&mut self, feed: F);
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
    build_all(config, keys, demo, cache, &mut launcher);
    launcher.started
}

/// Builds the feed for every source the config leaves on and hands each to `visitor`.
/// The one place that decides which feed serves which source, for running the rig
/// and for checking its config alike.
pub fn build_all(
    config: &Config,
    keys: &Keys,
    demo: bool,
    cache: Option<&Cache>,
    visitor: &mut impl Visit,
) {
    for source in SourceId::ALL {
        if disabled(config, source).is_some() {
            continue;
        }
        if demo {
            visitor.visit(demo::DemoFeed::new(source, config));
            continue;
        }
        match source {
            SourceId::Stocks => {
                let cached = cache.and_then(|c| c.load(SourceId::Stocks)).map(|(_, r)| r);
                visitor.visit(finnhub::Finnhub::new(
                    config,
                    keys.finnhub(),
                    cached.as_ref(),
                ));
            }
            SourceId::Crypto => {
                visitor.visit(coingecko::CoinGecko::new(config, keys.coingecko()));
            }
            SourceId::Weather => visitor.visit(open_meteo::OpenMeteo::new(config)),
            SourceId::Hn => visitor.visit(hn::HackerNews),
            SourceId::Kev => visitor.visit(kev::Kev),
            SourceId::Quakes => visitor.visit(usgs::Usgs::new(config)),
            SourceId::Swpc => visitor.visit(swpc::Swpc),
            SourceId::OpenSky => visitor.visit(opensky::OpenSky::new(config)),
            SourceId::Orbit => visitor.visit(orbit::Orbit::new(config)),
            SourceId::Rss => visitor.visit(rss::Rss::new(config)),
            SourceId::Ioda => visitor.visit(ioda::Ioda::new(config)),
            SourceId::Uplink => visitor.visit(uplink::Uplink::new()),
        }
    }
}

struct Launcher<'a> {
    http: &'a reqwest::Client,
    tx: &'a UnboundedSender<Msg>,
    rebreach: &'a broadcast::Sender<()>,
    started: Vec<(SourceId, Duration)>,
}

impl Visit for Launcher<'_> {
    fn visit<F: Feed>(&mut self, feed: F) {
        self.started.push((feed.source(), feed.interval()));
        spawn(
            feed,
            self.http.clone(),
            self.tx.clone(),
            self.rebreach.subscribe(),
        );
    }
}

/// How the config turns a source off, if it does: by listing nothing for it, or by
/// switching it off outright.
pub fn disabled(config: &Config, source: SourceId) -> Option<&'static str> {
    match source {
        SourceId::Stocks if config.zaibatsu.stocks.is_empty() => {
            Some("no [zaibatsu] stocks listed")
        }
        SourceId::Crypto if config.zaibatsu.coins.is_empty() => Some("no [zaibatsu] coins listed"),
        SourceId::Hn if !config.intercepts.hn => Some("[intercepts] hn = false"),
        SourceId::Kev if !config.intercepts.kev => Some("[intercepts] kev = false"),
        SourceId::Rss if config.intercepts.rss.is_empty() => {
            Some("no [intercepts] rss feeds listed")
        }
        _ => None,
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
            // Presses that queued up while this fetch ran are answered by it.
            loop {
                match rebreach.try_recv() {
                    Ok(()) | Err(TryRecvError::Lagged(_)) => {}
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Closed) => return,
                }
            }
            let rate_limited = matches!(result, Err(FetchError::RateLimited(_)));
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
            if !wait(retry_in, rate_limited, &mut rebreach).await {
                return;
            }
        }
    });
}

/// Sleeps until the next attempt is due or a re-breach comes in; `false` once the app
/// is gone. A rate limit is waited out in full, since breaching through it would only
/// earn another 429, or a longer ban.
async fn wait(
    retry_in: Option<Duration>,
    rate_limited: bool,
    rebreach: &mut broadcast::Receiver<()>,
) -> bool {
    let Some(wait) = retry_in else {
        return !matches!(rebreach.recv().await, Err(RecvError::Closed));
    };
    let due = tokio::time::sleep(wait);
    tokio::pin!(due);
    loop {
        tokio::select! {
            _ = &mut due => return true,
            r = rebreach.recv() => match r {
                Err(RecvError::Closed) => return false,
                _ if rate_limited => {}
                _ => return true,
            },
        }
    }
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;
    use crate::config::RssFeed;

    #[test]
    fn backoff_doubles_and_caps() {
        let base = Duration::from_secs(10);
        let first = backoff(base, 1);
        assert!(first >= Duration::from_secs(8) && first <= Duration::from_secs(12));
        let third = backoff(base, 3);
        assert!(third >= Duration::from_secs(32) && third <= Duration::from_secs(48));
        assert!(backoff(base, 40) <= MAX_BACKOFF.mul_f64(1.2));
    }

    /// Counts its fetches; each takes `takes` and ends in whatever `outcome` builds.
    struct Probe {
        fetches: Arc<AtomicU32>,
        takes: Duration,
        outcome: fn() -> Result<Reading, FetchError>,
    }

    impl Feed for Probe {
        fn source(&self) -> SourceId {
            SourceId::Swpc
        }

        fn interval(&self) -> Duration {
            Duration::from_secs(60)
        }

        async fn fetch(&self, _: &reqwest::Client) -> Result<Reading, FetchError> {
            self.fetches.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(self.takes).await;
            (self.outcome)()
        }
    }

    struct Running {
        fetches: Arc<AtomicU32>,
        rx: tokio::sync::mpsc::UnboundedReceiver<Msg>,
        rebreach: broadcast::Sender<()>,
    }

    impl Running {
        fn start(takes: Duration, outcome: fn() -> Result<Reading, FetchError>) -> Self {
            let fetches = Arc::new(AtomicU32::new(0));
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            let (rebreach, _) = broadcast::channel(4);
            let probe = Probe {
                fetches: fetches.clone(),
                takes,
                outcome,
            };
            spawn(probe, reqwest::Client::new(), tx, rebreach.subscribe());
            Self {
                fetches,
                rx,
                rebreach,
            }
        }

        fn fetches(&self) -> u32 {
            self.fetches.load(Ordering::SeqCst)
        }

        async fn until_done(&mut self) {
            while let Some(msg) = self.rx.recv().await {
                if matches!(msg, Msg::Feed(FeedMsg::Done { .. })) {
                    return;
                }
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn rebreach_waits_out_a_rate_limit() {
        let mut feed = Running::start(Duration::ZERO, || {
            Err(FetchError::RateLimited(Some(Duration::from_secs(600))))
        });
        feed.until_done().await;
        feed.rebreach.send(()).unwrap();
        tokio::time::sleep(Duration::from_secs(599)).await;
        assert_eq!(feed.fetches(), 1);
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert_eq!(feed.fetches(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn rebreaches_during_a_fetch_collapse_into_it() {
        let mut feed = Running::start(Duration::from_secs(5), || {
            Err(FetchError::Failed("reset".into()))
        });
        tokio::time::sleep(Duration::from_secs(1)).await;
        for _ in 0..3 {
            feed.rebreach.send(()).unwrap();
        }
        feed.until_done().await;
        // The first retry is 12-18s out, so nothing else should have started.
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert_eq!(feed.fetches(), 1);
        // A fresh press still cuts an ordinary backoff short.
        feed.rebreach.send(()).unwrap();
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert_eq!(feed.fetches(), 2);
    }

    #[test]
    fn intercepts_sources_can_be_individually_suppressed() {
        let mut config = Config::default();
        assert!(disabled(&config, SourceId::Hn).is_none());
        assert!(disabled(&config, SourceId::Kev).is_none());
        assert!(disabled(&config, SourceId::Rss).is_some());

        config.intercepts.hn = false;
        config.intercepts.kev = false;
        config.intercepts.rss = vec![RssFeed::new("https://example.com/feed.xml")];
        assert!(disabled(&config, SourceId::Hn).is_some());
        assert!(disabled(&config, SourceId::Kev).is_some());
        assert!(disabled(&config, SourceId::Rss).is_none());
    }
}
