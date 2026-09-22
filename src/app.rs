use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::broadcast;

use crate::cache::Cache;
use crate::config::Config;
use crate::cpu::CpuMeter;
use crate::dive::{Change, DiveCycle};
use crate::event::Msg;
use crate::feeds::{FeedMsg, FetchError};
use crate::intercept;
use crate::reading::Reading;
use crate::source::{FLATLINE_AFTER, Link, NodeId, SourceId, SourceState, best_link};

/// Frame interval while a fetch spinner is on screen.
const BUSY_FRAME: Duration = Duration::from_millis(100);
/// How long a priority intercept stays in the ticker.
const INTERCEPT_SHOWN: Duration = Duration::from_secs(30);

/// Things the effects layer reacts to, drained once per frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// New data landed.
    Updated(NodeId),
    /// A priority intercept fired.
    Intercept(NodeId),
    /// A source fell into ICE, TRACE, or FLATLINED.
    Trouble(NodeId),
    Dived(NodeId),
    Surfaced,
}

pub struct Intercept {
    pub node: NodeId,
    pub text: String,
    pub at: Instant,
}

pub struct App {
    pub config: Config,
    pub config_found: bool,
    pub demo: bool,
    /// Every source that was started, keyed for stable ticker order.
    pub sources: BTreeMap<SourceId, SourceState>,
    pub readings: HashMap<SourceId, Reading>,
    pub cpu: CpuMeter,
    pub dive: DiveCycle,
    pub intercept: Option<Intercept>,
    pub should_quit: bool,
    /// Standing problems found at startup, shown in the ticker until fixed.
    pub warnings: Vec<String>,
    signals: Vec<Signal>,
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
        let dive = DiveCycle::new(config.fx.dive_every, config.fx.dive_hold, Instant::now());
        Self {
            config,
            config_found,
            demo,
            sources,
            readings,
            cpu: CpuMeter::new(),
            dive,
            intercept: None,
            should_quit: false,
            warnings: Vec::new(),
            signals: Vec::new(),
            rebreach,
            cache,
        }
    }

    pub fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Key(key) => self.handle_key(key, Instant::now()),
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

    fn handle_key(&mut self, key: KeyEvent, now: Instant) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            // Esc backs out of a dive first, then out of the rig.
            KeyCode::Esc => match self.dive.diving() {
                Some(_) => {
                    self.dive.surface(now);
                    self.signals.push(Signal::Surfaced);
                }
                None => self.should_quit = true,
            },
            KeyCode::Char('r') => {
                // No receivers only means no sources are running; nothing to wake.
                let _ = self.rebreach.send(());
            }
            KeyCode::Char(' ') => {
                let ready = self.ready_nodes();
                let change = self.dive.advance(now, |n| ready.contains(&n));
                self.push_change(change);
            }
            KeyCode::Char('p') => self.dive.toggle_hold(now),
            KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                let idx = c as usize - '1' as usize;
                if let Some(&node) = NodeId::ALL.get(idx)
                    && self.node_ready(node)
                    && self.dive.diving() != Some(node)
                {
                    let change = self.dive.dive_now(node, now);
                    self.push_change(Some(change));
                }
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
        let node = source.node();
        let Some(state) = self.sources.get_mut(&source) else {
            return;
        };
        let was_troubled = is_trouble(&state.link);
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
                let events = intercept::detect(
                    self.readings.get(&source),
                    &reading,
                    &self.config.sector,
                    now,
                );
                self.readings.insert(source, reading);
                self.signals.push(Signal::Updated(node));
                if !events.is_empty() {
                    self.raise_intercept(node, events.join(" · "));
                }
            }
            Err(error) => {
                state.link = match error {
                    FetchError::Failed(e) => {
                        state.last_error = Some(e);
                        if failures >= FLATLINE_AFTER {
                            Link::Flatlined
                        } else {
                            Link::Ice
                        }
                    }
                    FetchError::RateLimited(_) => Link::Trace,
                    FetchError::NotConfigured(reason) => Link::Offline(reason),
                };
                if !was_troubled && is_trouble(&state.link) {
                    self.signals.push(Signal::Trouble(node));
                }
            }
        }
    }

    fn raise_intercept(&mut self, node: NodeId, text: String) {
        let now = Instant::now();
        tracing::info!(?node, "priority intercept: {text}");
        self.dive.prioritize(node, now);
        self.intercept = Some(Intercept {
            node,
            text,
            at: now,
        });
        self.signals.push(Signal::Intercept(node));
    }

    fn push_change(&mut self, change: Option<Change>) {
        match change {
            Some(Change::Dived(node)) => self.signals.push(Signal::Dived(node)),
            Some(Change::Surfaced) => self.signals.push(Signal::Surfaced),
            None => {}
        }
    }

    pub fn on_tick(&mut self, now: Instant) {
        self.cpu.sample();
        let ready = self.ready_nodes();
        let change = self.dive.tick(now, |n| ready.contains(&n));
        self.push_change(change);
    }

    pub fn take_signals(&mut self) -> Vec<Signal> {
        std::mem::take(&mut self.signals)
    }

    /// The current intercept, while it's recent enough to show.
    pub fn live_intercept(&self, now: Instant) -> Option<&Intercept> {
        self.intercept
            .as_ref()
            .filter(|i| now.saturating_duration_since(i.at) < INTERCEPT_SHOWN)
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

    /// Whether a node has any data to show (and so is worth diving into).
    pub fn node_ready(&self, node: NodeId) -> bool {
        node.sources()
            .iter()
            .any(|id| self.readings.contains_key(id))
    }

    pub fn ready_nodes(&self) -> Vec<NodeId> {
        NodeId::ALL
            .into_iter()
            .filter(|n| self.node_ready(*n))
            .collect()
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

fn is_trouble(link: &Link) -> bool {
    matches!(link, Link::Ice | Link::Trace | Link::Flatlined)
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyEvent;

    use super::*;
    use crate::reading::Quote;

    fn app(sources: &[SourceId]) -> App {
        let started = sources
            .iter()
            .map(|id| (*id, Duration::from_secs(60)))
            .collect();
        App::new(
            Config::default(),
            true,
            true,
            started,
            broadcast::channel(1).0,
            None,
        )
    }

    fn done(source: SourceId, result: Result<Reading, FetchError>, failures: u32) -> Msg {
        Msg::Feed(FeedMsg::Done {
            source,
            result,
            failures,
            interval: Duration::from_secs(60),
            retry_in: Some(Duration::from_secs(30)),
        })
    }

    fn crypto(change_pct: f64) -> Reading {
        Reading::Crypto(vec![Quote {
            symbol: "BTC".into(),
            price: 1.0,
            change_pct,
            spark: Vec::new(),
        }])
    }

    fn key(code: KeyCode) -> Msg {
        Msg::Key(KeyEvent::from(code))
    }

    #[test]
    fn updates_and_intercepts_signal_the_effects_layer() {
        let mut app = app(&[SourceId::Crypto]);
        app.handle(done(SourceId::Crypto, Ok(crypto(1.0)), 0));
        assert_eq!(app.take_signals(), [Signal::Updated(NodeId::Zaibatsu)]);
        app.handle(done(SourceId::Crypto, Ok(crypto(4.2)), 0));
        assert_eq!(
            app.take_signals(),
            [
                Signal::Updated(NodeId::Zaibatsu),
                Signal::Intercept(NodeId::Zaibatsu)
            ]
        );
        let intercept = app.live_intercept(Instant::now()).unwrap();
        assert_eq!(intercept.text, "BTC ▲4.2%");
        assert_eq!(app.dive.peek(|_| true), Some(NodeId::Zaibatsu));
    }

    #[test]
    fn trouble_signals_only_on_the_way_in() {
        let mut app = app(&[SourceId::Swpc]);
        let fail = || Err(FetchError::Failed("reset".into()));
        app.handle(done(SourceId::Swpc, fail(), 1));
        assert_eq!(app.take_signals(), [Signal::Trouble(NodeId::Seismic)]);
        app.handle(done(SourceId::Swpc, fail(), 2));
        assert!(app.take_signals().is_empty());
        app.handle(done(SourceId::Swpc, fail(), 3));
        assert_eq!(app.sources[&SourceId::Swpc].link, Link::Flatlined);
    }

    #[test]
    fn keys_dive_surface_and_quit() {
        let mut app = app(&[SourceId::Crypto]);
        // Nothing to show yet, so no dive.
        app.handle(key(KeyCode::Char('1')));
        assert_eq!(app.dive.diving(), None);
        app.handle(done(SourceId::Crypto, Ok(crypto(1.0)), 0));
        app.take_signals();

        app.handle(key(KeyCode::Char('1')));
        assert_eq!(app.dive.diving(), Some(NodeId::Zaibatsu));
        assert_eq!(app.take_signals(), [Signal::Dived(NodeId::Zaibatsu)]);
        app.handle(key(KeyCode::Esc));
        assert_eq!(app.dive.diving(), None);
        assert!(!app.should_quit);
        app.handle(key(KeyCode::Esc));
        assert!(app.should_quit);
    }
}
