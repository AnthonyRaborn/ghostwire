use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::broadcast;

use crate::cache::Cache;
use crate::config::Config;
use crate::cpu::CpuMeter;
use crate::event::Msg;
use crate::feeds::{FeedMsg, FetchError};
use crate::reading::Reading;
use crate::source::{FLATLINE_AFTER, Link, NodeId, SourceId, SourceState, best_link};

/// Frame interval while a fetch spinner is on screen.
const BUSY_FRAME: Duration = Duration::from_millis(100);

pub struct App {
    pub config: Config,
    pub config_found: bool,
    pub demo: bool,
    /// Every source that was started, keyed for stable ticker order.
    pub sources: BTreeMap<SourceId, SourceState>,
    pub readings: HashMap<SourceId, Reading>,
    pub cpu: CpuMeter,
    pub should_quit: bool,
    rebreach: broadcast::Sender<()>,
    /// `None` in construct mode, so simulated data never lands on disk.
    cache: Option<Cache>,
}

impl App {
    pub fn new(
        config: Config,
        config_found: bool,
        demo: bool,
        started: Vec<(SourceId, Duration)>,
        rebreach: broadcast::Sender<()>,
        cache: Option<Cache>,
    ) -> Self {
        let mut sources = BTreeMap::new();
        let mut readings = HashMap::new();
        for (id, interval) in started {
            let mut state = SourceState::new(interval);
            if let Some((fetched_at, reading)) = cache.as_ref().and_then(|c| c.load(id)) {
                state.link = Link::Ghost;
                state.last_ok = Some(fetched_at);
                readings.insert(id, reading);
            }
            sources.insert(id, state);
        }
        Self {
            config,
            config_found,
            demo,
            sources,
            readings,
            cpu: CpuMeter::new(),
            should_quit: false,
            rebreach,
            cache,
        }
    }

    pub fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Key(key) => self.handle_key(key),
            Msg::Resize => {}
            Msg::Feed(FeedMsg::Attempt(id)) => {
                if let Some(state) = self.sources.get_mut(&id) {
                    state.in_flight = true;
                }
            }
            Msg::Feed(FeedMsg::Done {
                source,
                result,
                failures,
                interval,
                retry_in,
            }) => self.apply(source, result, failures, interval, retry_in),
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Char('r') => {
                // No receivers only means no sources are running; nothing to wake.
                let _ = self.rebreach.send(());
            }
            _ => {}
        }
    }

    fn apply(
        &mut self,
        source: SourceId,
        result: Result<Reading, FetchError>,
        failures: u32,
        interval: Duration,
        retry_in: Option<Duration>,
    ) {
        let Some(state) = self.sources.get_mut(&source) else {
            return;
        };
        state.in_flight = false;
        state.failures = failures;
        state.next_attempt = retry_in.map(|wait| Instant::now() + wait);
        match result {
            Ok(reading) => {
                let now = Utc::now();
                state.link = Link::Live;
                state.last_ok = Some(now);
                state.last_error = None;
                state.interval = interval;
                if let Some(cache) = &self.cache {
                    cache.store(source, now, &reading);
                }
                self.readings.insert(source, reading);
            }
            Err(FetchError::Failed(error)) => {
                state.link = if failures >= FLATLINE_AFTER {
                    Link::Flatlined
                } else {
                    Link::Ice
                };
                state.last_error = Some(error);
            }
            Err(FetchError::RateLimited(_)) => state.link = Link::Trace,
            Err(FetchError::NotConfigured(reason)) => state.link = Link::Offline(reason),
        }
    }

    pub fn on_tick(&mut self) {
        self.cpu.sample();
    }

    /// How long the main loop may sleep before drawing the next frame.
    pub fn frame_wait(&self) -> Duration {
        if self.sources.values().any(|s| s.in_flight) {
            return BUSY_FRAME;
        }
        // Otherwise wake just after the next wall-clock second so the clock ticks cleanly.
        let ms = Local::now().timestamp_subsec_millis().min(999);
        Duration::from_millis(u64::from(1_000 - ms) + 5)
    }

    pub fn node_sources(&self, node: NodeId) -> impl Iterator<Item = (SourceId, &SourceState)> {
        node.sources()
            .iter()
            .filter_map(|id| self.sources.get(id).map(|state| (*id, state)))
    }

    pub fn node_link(&self, node: NodeId) -> Option<Link> {
        best_link(self.node_sources(node).map(|(_, s)| s))
    }

    pub fn node_busy(&self, node: NodeId) -> bool {
        self.node_sources(node).any(|(_, s)| s.in_flight)
    }

    /// The most recent successful fetch across the node's sources.
    pub fn node_last_ok(&self, node: NodeId) -> Option<DateTime<Utc>> {
        self.node_sources(node).filter_map(|(_, s)| s.last_ok).max()
    }

    /// The freshest source's decay, so a node only dims once all of it is stale.
    pub fn node_decay(&self, node: NodeId, now: DateTime<Utc>) -> f32 {
        self.node_sources(node)
            .filter(|(_, s)| s.last_ok.is_some())
            .map(|(_, s)| s.decay(now))
            .reduce(f32::min)
            .unwrap_or(0.0)
    }

    /// `(live, total)` sources.
    pub fn uplink(&self) -> (usize, usize) {
        let live = self
            .sources
            .values()
            .filter(|s| s.link == Link::Live)
            .count();
        (live, self.sources.len())
    }
}
