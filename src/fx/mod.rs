//! Effects applied to the rendered buffer after widgets draw. Every effect is a pure
//! function of cell position and elapsed time, so any frame can be reproduced exactly
//! (and tested). Which effects run, and how long, depends on the FX level.

mod boot;
mod decrypt;
mod glitch;
mod noise;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use boot::Boot;
pub use boot::BootLine;
use decrypt::Decrypt;
use glitch::Burst;

use crate::app::Signal;
use crate::colordepth::{self, Depth};
use crate::config::FxLevel;
use crate::source::NodeId;
use crate::theme;

/// Frame interval while anything is moving.
const ANIMATING_FRAME: Duration = Duration::from_millis(33);
/// Chaotic mode never fully rests; its background noise ticks at this rate.
const CHAOTIC_IDLE_FRAME: Duration = Duration::from_millis(100);
const AMBIENT_BURST: Duration = Duration::from_millis(160);

struct Tuning {
    /// Per-line typing time for the boot log; `None` skips the boot.
    boot_line: Option<Duration>,
    /// Scramble on refreshed values.
    decrypt: Duration,
    /// Scramble on a whole node: after boot, on a dive, on surfacing.
    reveal: Duration,
    /// Glitch when a source falls into ICE / TRACE / FLATLINED.
    trouble_burst: Option<Duration>,
    intercept_burst: Duration,
    /// Seconds between small unprompted glitches, as `lo..hi`.
    ambient: Option<(u64, u64)>,
    decay_noise: bool,
    sparkle: bool,
}

fn tuning(level: FxLevel) -> Tuning {
    let ms = Duration::from_millis;
    match level {
        FxLevel::Calm => Tuning {
            boot_line: None,
            decrypt: ms(300),
            reveal: ms(300),
            trouble_burst: None,
            intercept_burst: ms(300),
            ambient: None,
            decay_noise: false,
            sparkle: false,
        },
        FxLevel::Active => Tuning {
            boot_line: Some(ms(260)),
            decrypt: ms(700),
            reveal: ms(600),
            trouble_burst: Some(ms(300)),
            intercept_burst: ms(700),
            ambient: Some((20, 40)),
            decay_noise: true,
            sparkle: false,
        },
        FxLevel::Chaotic => Tuning {
            boot_line: Some(ms(380)),
            decrypt: ms(1_000),
            reveal: ms(900),
            trouble_burst: Some(ms(450)),
            intercept_burst: ms(1_000),
            ambient: Some((2, 6)),
            decay_noise: true,
            sparkle: true,
        },
    }
}

/// What a node's body looked like last frame, as one hash per cell, before any effect.
#[derive(Clone)]
pub struct Snapshot {
    area: Rect,
    cells: Vec<u64>,
}

impl Snapshot {
    fn take(buf: &Buffer, area: Rect, reuse: Option<Snapshot>) -> Self {
        let mut cells = reuse.map(|s| s.cells).unwrap_or_default();
        cells.clear();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                cells.push(buf.cell((x, y)).map_or(0, |c| symbol_hash(c.symbol())));
            }
        }
        Self { area, cells }
    }

    /// Whether the cell at `index` of `area` differs from this snapshot.
    fn changed(&self, area: Rect, index: usize, symbol: &str) -> bool {
        self.area != area || self.cells.get(index) != Some(&symbol_hash(symbol))
    }
}

pub struct Fx {
    tuning: Tuning,
    depth: Depth,
    epoch: Instant,
    boot: Option<Boot>,
    decrypts: HashMap<NodeId, Decrypt>,
    bursts: HashMap<NodeId, Burst>,
    snapshots: HashMap<NodeId, Snapshot>,
    next_ambient: Option<Instant>,
}

impl Fx {
    pub fn new(level: FxLevel, boot_lines: Vec<BootLine>, now: Instant, depth: Depth) -> Self {
        let tuning = tuning(level);
        let boot = tuning
            .boot_line
            .map(|per_line| Boot::new(boot_lines, per_line, now));
        let next_ambient = tuning.ambient.map(|range| now + ambient_wait(range, 0));
        Self {
            tuning,
            depth,
            epoch: now,
            boot,
            decrypts: HashMap::new(),
            bursts: HashMap::new(),
            snapshots: HashMap::new(),
            next_ambient,
        }
    }

    pub fn booting(&self, now: Instant) -> bool {
        self.boot.as_ref().is_some_and(|b| !b.done(now))
    }

    pub fn skip_boot(&mut self, now: Instant) {
        if let Some(boot) = &mut self.boot {
            boot.skip(now);
        }
    }

    pub fn draw_boot(&self, frame: &mut Frame, now: Instant, rig_id: &str) {
        if let Some(boot) = &self.boot {
            boot.draw(frame, frame.area(), now, self.ms(now), rig_id);
        }
    }

    pub fn absorb(&mut self, signals: impl IntoIterator<Item = Signal>, now: Instant) {
        for signal in signals {
            match signal {
                Signal::Updated(node) => {
                    let before = self.snapshots.get(&node).cloned();
                    let decrypt = Decrypt::new(now, self.tuning.decrypt, before);
                    self.decrypts.insert(node, decrypt);
                }
                Signal::Intercept(node) => {
                    self.bursts
                        .insert(node, Burst::new(now, self.tuning.intercept_burst, 1.0));
                }
                Signal::Trouble(node) => {
                    if let Some(duration) = self.tuning.trouble_burst {
                        self.bursts.insert(node, Burst::new(now, duration, 0.6));
                    }
                }
                Signal::Dived(node) => self.reveal(node, now),
                Signal::Surfaced => NodeId::ALL.into_iter().for_each(|n| self.reveal(n, now)),
            }
        }
    }

    fn reveal(&mut self, node: NodeId, now: Instant) {
        self.decrypts
            .insert(node, Decrypt::new(now, self.tuning.reveal, None));
    }

    /// Housekeeping before each frame: finish the boot with a reveal, schedule ambient
    /// glitches, and drop finished effects. `ready` lists nodes with something on screen.
    pub fn on_frame(&mut self, now: Instant, ready: &[NodeId]) {
        if self.boot.as_ref().is_some_and(|b| b.done(now)) {
            self.boot = None;
            NodeId::ALL.into_iter().for_each(|n| self.reveal(n, now));
        }
        if let (Some(at), Some(range)) = (self.next_ambient, self.tuning.ambient)
            && now >= at
        {
            let ms = self.ms(now);
            if !ready.is_empty() {
                let node = ready[(hash(&[ms, 0xA11]) % ready.len() as u64) as usize];
                self.bursts
                    .entry(node)
                    .or_insert_with(|| Burst::new(now, AMBIENT_BURST, 0.4));
            }
            self.next_ambient = Some(now + ambient_wait(range, ms));
        }
        self.decrypts.retain(|_, d| !d.done(now));
        self.bursts.retain(|_, b| !b.done(now));
    }

    /// How long the loop may sleep; `Duration::MAX` when nothing is moving.
    pub fn frame_wait(&self, now: Instant, radar_on_screen: bool) -> Duration {
        let moving = self.booting(now)
            || radar_on_screen
            || self.decrypts.values().any(|d| !d.done(now))
            || self.bursts.values().any(|b| !b.done(now));
        if moving {
            ANIMATING_FRAME
        } else if self.tuning.sparkle {
            CHAOTIC_IDLE_FRAME
        } else {
            Duration::MAX
        }
    }

    /// Runs a node's effects over what its widgets just drew. `outer` includes the
    /// border (glitches tear it too); `inner` is the body.
    pub fn node(
        &mut self,
        buf: &mut Buffer,
        outer: Rect,
        inner: Rect,
        node: NodeId,
        now: Instant,
        decay: f32,
    ) {
        let inner = inner.intersection(buf.area);
        let previous = self.snapshots.remove(&node);
        self.snapshots
            .insert(node, Snapshot::take(buf, inner, previous));
        let ms = self.ms(now);
        if let Some(d) = self.decrypts.get(&node) {
            d.apply(buf, inner, now);
        }
        if self.tuning.decay_noise && decay > 0.0 {
            noise::decay(buf, inner, decay, ms);
        }
        if self.tuning.sparkle {
            noise::sparkle(buf, inner, ms);
        }
        if let Some(b) = self.bursts.get(&node) {
            b.apply(buf, outer.intersection(buf.area), now);
        }
    }

    /// Whole-screen effects, applied last.
    pub fn screen(&self, buf: &mut Buffer, area: Rect) {
        noise::scanlines(buf, area);
        colordepth::downsample(buf, area, self.depth);
    }

    fn ms(&self, now: Instant) -> u64 {
        now.saturating_duration_since(self.epoch).as_millis() as u64
    }
}

fn ambient_wait((lo, hi): (u64, u64), salt: u64) -> Duration {
    let span = hi.saturating_sub(lo).max(1);
    Duration::from_secs(lo + hash(&[salt, 0xB0B]) % span)
}

/// Signal decay: pull every foreground color in `area` toward the dim background tone.
pub fn fade(buf: &mut Buffer, area: Rect, amount: f32) {
    let area = area.intersection(buf.area);
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.fg = theme::mix(cell.fg, theme::DIM, amount);
            }
        }
    }
}

fn splitmix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A well-mixed hash of a few integers: the effects' only source of randomness.
fn hash(parts: &[u64]) -> u64 {
    parts.iter().fold(0x5EED, |h, &p| splitmix(h ^ p))
}

/// Maps a hash onto `0.0..1.0`.
fn unit(h: u64) -> f32 {
    (h >> 40) as f32 / (1u64 << 24) as f32
}

fn symbol_hash(symbol: &str) -> u64 {
    symbol.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    })
}

/// Fraction of `duration` elapsed since `start`, clamped to `0.0..=1.0`.
fn progress(start: Instant, duration: Duration, now: Instant) -> f32 {
    if duration.is_zero() {
        return 1.0;
    }
    (now.saturating_duration_since(start).as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_deterministic_and_spread() {
        assert_eq!(hash(&[1, 2, 3]), hash(&[1, 2, 3]));
        assert_ne!(hash(&[1, 2, 3]), hash(&[3, 2, 1]));
        let mean = (0..10_000).map(|i| unit(hash(&[i])) as f64).sum::<f64>() / 10_000.0;
        assert!((mean - 0.5).abs() < 0.02, "{mean}");
    }

    #[test]
    fn rests_when_nothing_moves() {
        let now = Instant::now();
        let mut fx = Fx::new(FxLevel::Active, Vec::new(), now, Depth::TrueColor);
        fx.skip_boot(now);
        fx.on_frame(now, &[]);
        // The boot's closing reveal is still running.
        assert_eq!(fx.frame_wait(now, false), ANIMATING_FRAME);
        let later = now + Duration::from_secs(2);
        fx.on_frame(later, &[]);
        assert_eq!(fx.frame_wait(later, false), Duration::MAX);
        assert_eq!(fx.frame_wait(later, true), ANIMATING_FRAME);
    }

    #[test]
    fn calm_skips_the_boot_and_never_glitches_on_trouble() {
        let now = Instant::now();
        let mut fx = Fx::new(
            FxLevel::Calm,
            vec![BootLine::new("x", "y")],
            now,
            Depth::TrueColor,
        );
        assert!(!fx.booting(now));
        fx.absorb([Signal::Trouble(NodeId::Sky)], now);
        assert!(fx.bursts.is_empty());
        fx.absorb([Signal::Intercept(NodeId::Sky)], now);
        assert_eq!(fx.bursts.len(), 1);
    }
}
