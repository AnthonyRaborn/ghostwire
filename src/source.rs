//! Identity of every node and source, and the per-source link-status model.

use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};

/// A panel on screen. Each node shows the combined readings of one or more sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NodeId {
    Zaibatsu,
    Atmos,
    Intercepts,
    /// Quakes and space weather: both are low-density feeds, so they share a node.
    Seismic,
    Sky,
}

impl NodeId {
    pub const ALL: [NodeId; 5] = [
        NodeId::Zaibatsu,
        NodeId::Atmos,
        NodeId::Intercepts,
        NodeId::Seismic,
        NodeId::Sky,
    ];

    pub fn sources(self) -> &'static [SourceId] {
        match self {
            NodeId::Zaibatsu => &[SourceId::Stocks, SourceId::Crypto],
            NodeId::Atmos => &[SourceId::Weather],
            NodeId::Intercepts => &[SourceId::Kev, SourceId::Hn],
            NodeId::Seismic => &[SourceId::Quakes, SourceId::Swpc],
            NodeId::Sky => &[SourceId::OpenSky],
        }
    }
}

/// One upstream feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SourceId {
    Stocks,
    Crypto,
    Weather,
    Hn,
    Kev,
    Quakes,
    Swpc,
    OpenSky,
}

impl SourceId {
    pub const ALL: [SourceId; 8] = [
        SourceId::Stocks,
        SourceId::Crypto,
        SourceId::Weather,
        SourceId::Hn,
        SourceId::Kev,
        SourceId::Quakes,
        SourceId::Swpc,
        SourceId::OpenSky,
    ];

    pub fn node(self) -> NodeId {
        NodeId::ALL
            .into_iter()
            .find(|n| n.sources().contains(&self))
            .expect("every source belongs to a node")
    }

    /// The upstream service's name, as shown in the footer ticker.
    pub fn handle(self) -> &'static str {
        match self {
            SourceId::Stocks => "FINNHUB",
            SourceId::Crypto => "COINGECKO",
            SourceId::Weather => "OPEN-METEO",
            SourceId::Hn => "HN",
            SourceId::Kev => "CISA-KEV",
            SourceId::Quakes => "USGS",
            SourceId::Swpc => "NOAA-SWPC",
            SourceId::OpenSky => "OPENSKY",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Link {
    /// No fetch has finished yet this session.
    Pending,
    Live,
    /// Showing a cached reading from a previous session; no live fetch has landed yet.
    Ghost,
    /// The last fetch failed; retrying with backoff.
    Ice,
    /// Rate-limited upstream; dark until the backoff window passes.
    Trace,
    /// `FLATLINE_AFTER` consecutive failures. Still retrying, just slower.
    Flatlined,
    /// Can't run until config changes (missing key, no location).
    Offline(String),
}

impl Link {
    /// Lower is healthier. A node shows the healthiest of its sources.
    fn rank(&self) -> u8 {
        match self {
            Link::Live => 0,
            Link::Ghost => 1,
            Link::Pending => 2,
            Link::Ice => 3,
            Link::Trace => 4,
            Link::Flatlined => 5,
            Link::Offline(_) => 6,
        }
    }
}

pub const FLATLINE_AFTER: u32 = 3;

/// Data this many refresh intervals old starts to decay…
const DECAY_START: f32 = 1.5;
/// …and is fully decayed by this many.
const DECAY_FULL: f32 = 6.0;

#[derive(Debug, Clone)]
pub struct SourceState {
    pub link: Link,
    pub in_flight: bool,
    pub failures: u32,
    pub last_ok: Option<DateTime<Utc>>,
    pub next_attempt: Option<Instant>,
    pub last_error: Option<String>,
    /// The normal refresh interval, used to judge staleness.
    pub interval: Duration,
}

impl SourceState {
    pub fn new(interval: Duration) -> Self {
        Self {
            link: Link::Pending,
            in_flight: false,
            failures: 0,
            last_ok: None,
            next_attempt: None,
            last_error: None,
            interval,
        }
    }

    /// 0.0 while fresh, rising to 1.0 as the data ages past several refresh intervals.
    pub fn decay(&self, now: DateTime<Utc>) -> f32 {
        let Some(last) = self.last_ok else { return 0.0 };
        let age = (now - last).to_std().unwrap_or_default().as_secs_f32();
        let intervals = age / self.interval.as_secs_f32().max(1.0);
        ((intervals - DECAY_START) / (DECAY_FULL - DECAY_START)).clamp(0.0, 1.0)
    }

    /// Whole seconds until the next scheduled attempt.
    pub fn retry_in(&self, now: Instant) -> Option<u64> {
        self.next_attempt
            .map(|at| at.saturating_duration_since(now).as_secs_f32().ceil() as u64)
    }
}

/// The healthiest link among `states`, or `None` if there are none.
pub fn best_link<'a>(states: impl IntoIterator<Item = &'a SourceState>) -> Option<Link> {
    states
        .into_iter()
        .map(|s| &s.link)
        .min_by_key(|l| l.rank())
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_link(link: Link) -> SourceState {
        SourceState {
            link,
            ..SourceState::new(Duration::from_secs(60))
        }
    }

    #[test]
    fn node_shows_healthiest_source() {
        let states = [
            with_link(Link::Offline("no key".into())),
            with_link(Link::Live),
        ];
        assert_eq!(best_link(&states), Some(Link::Live));
        let states = [with_link(Link::Trace), with_link(Link::Ice)];
        assert_eq!(best_link(&states), Some(Link::Ice));
        assert_eq!(best_link(&[]), None);
    }

    #[test]
    fn every_source_belongs_to_exactly_one_node() {
        for source in SourceId::ALL {
            let owners = NodeId::ALL
                .iter()
                .filter(|n| n.sources().contains(&source))
                .count();
            assert_eq!(owners, 1, "{source:?}");
        }
    }

    #[test]
    fn decay_ramps_with_age() {
        let now = Utc::now();
        let mut state = SourceState::new(Duration::from_secs(60));
        assert_eq!(state.decay(now), 0.0);
        state.last_ok = Some(now - chrono::Duration::seconds(60));
        assert_eq!(state.decay(now), 0.0);
        state.last_ok = Some(now - chrono::Duration::seconds(60 * 4));
        let mid = state.decay(now);
        assert!(mid > 0.0 && mid < 1.0);
        state.last_ok = Some(now - chrono::Duration::seconds(60 * 10));
        assert_eq!(state.decay(now), 1.0);
    }
}
