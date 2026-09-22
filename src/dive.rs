//! The dive cycle. Every `every`, the rig dives into one node: it takes over the grid
//! for `hold`, then surfaces. Nodes take turns in grid order, skipping any with nothing
//! to show; a priority intercept jumps the queue and pulls the next dive forward.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::source::NodeId;

/// How soon a priority intercept brings on the next dive.
const PRIORITY_LEAD: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Grid { next_at: Instant },
    Diving { node: NodeId, until: Instant },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Dived(NodeId),
    Surfaced,
}

pub struct DiveCycle {
    hold: Duration,
    /// Time on the grid between dives.
    gap: Duration,
    phase: Phase,
    /// While held, nothing changes on its own; manual dives and surfacing still work.
    held: bool,
    queue: VecDeque<NodeId>,
    /// Index into `NodeId::ALL` of the last node the rotation dived into.
    cursor: usize,
}

impl DiveCycle {
    /// `hold` must be shorter than `every` (the config enforces it).
    pub fn new(every: Duration, hold: Duration, now: Instant) -> Self {
        let gap = every.saturating_sub(hold);
        Self {
            hold,
            gap,
            phase: Phase::Grid { next_at: now + gap },
            held: false,
            queue: VecDeque::new(),
            cursor: NodeId::ALL.len() - 1,
        }
    }

    pub fn diving(&self) -> Option<NodeId> {
        match self.phase {
            Phase::Diving { node, .. } => Some(node),
            Phase::Grid { .. } => None,
        }
    }

    pub fn held(&self) -> bool {
        self.held
    }

    /// Time until the next dive or surfacing; `None` while held.
    pub fn next_in(&self, now: Instant) -> Option<Duration> {
        if self.held {
            return None;
        }
        let at = match self.phase {
            Phase::Grid { next_at } => next_at,
            Phase::Diving { until, .. } => until,
        };
        Some(at.saturating_duration_since(now))
    }

    /// The node the next dive would pick.
    pub fn peek(&self, ready: impl Fn(NodeId) -> bool) -> Option<NodeId> {
        self.queue
            .iter()
            .copied()
            .find(|n| ready(*n))
            .or_else(|| self.rotation_next(&ready).map(|(_, n)| n))
    }

    /// Advances on its own schedule. `ready` says whether a node has anything to show.
    pub fn tick(&mut self, now: Instant, ready: impl Fn(NodeId) -> bool) -> Option<Change> {
        if self.held {
            return None;
        }
        match self.phase {
            Phase::Diving { until, .. } if now >= until => {
                self.surface(now);
                Some(Change::Surfaced)
            }
            Phase::Grid { next_at } if now >= next_at => match self.pick(&ready) {
                Some(node) => {
                    self.enter(node, now);
                    Some(Change::Dived(node))
                }
                None => {
                    self.phase = Phase::Grid {
                        next_at: now + self.gap,
                    };
                    None
                }
            },
            _ => None,
        }
    }

    /// Dives straight into `node`, whatever's on screen.
    pub fn dive_now(&mut self, node: NodeId, now: Instant) -> Change {
        self.enter(node, now);
        Change::Dived(node)
    }

    /// Space bar: surface if diving, otherwise dive into the next node.
    pub fn advance(&mut self, now: Instant, ready: impl Fn(NodeId) -> bool) -> Option<Change> {
        if self.diving().is_some() {
            self.surface(now);
            return Some(Change::Surfaced);
        }
        let node = self.pick(&ready)?;
        Some(self.dive_now(node, now))
    }

    pub fn surface(&mut self, now: Instant) {
        let wait = if self.queue.is_empty() {
            self.gap
        } else {
            PRIORITY_LEAD
        };
        self.phase = Phase::Grid {
            next_at: now + wait,
        };
    }

    /// Unholding restarts the current phase's timer, so nothing fires the instant
    /// the hold lifts.
    pub fn toggle_hold(&mut self, now: Instant) {
        self.held = !self.held;
        if !self.held {
            self.phase = match self.phase {
                Phase::Grid { .. } => Phase::Grid {
                    next_at: now + self.gap,
                },
                Phase::Diving { node, .. } => Phase::Diving {
                    node,
                    until: now + self.hold,
                },
            };
        }
    }

    /// Something notable happened on `node`: dive into it next, and soon.
    pub fn prioritize(&mut self, node: NodeId, now: Instant) {
        if let Phase::Diving {
            node: current,
            until,
        } = &mut self.phase
            && *current == node
        {
            *until = (*until).max(now + self.hold);
            return;
        }
        if !self.queue.contains(&node) {
            self.queue.push_back(node);
        }
        if let Phase::Grid { next_at } = &mut self.phase {
            *next_at = (*next_at).min(now + PRIORITY_LEAD);
        }
    }

    fn enter(&mut self, node: NodeId, now: Instant) {
        self.queue.retain(|n| *n != node);
        self.phase = Phase::Diving {
            node,
            until: now + self.hold,
        };
    }

    fn pick(&mut self, ready: &impl Fn(NodeId) -> bool) -> Option<NodeId> {
        while let Some(node) = self.queue.pop_front() {
            if ready(node) {
                return Some(node);
            }
        }
        let (index, node) = self.rotation_next(ready)?;
        self.cursor = index;
        Some(node)
    }

    fn rotation_next(&self, ready: &impl Fn(NodeId) -> bool) -> Option<(usize, NodeId)> {
        let n = NodeId::ALL.len();
        (1..=n)
            .map(|step| (self.cursor + step) % n)
            .map(|i| (i, NodeId::ALL[i]))
            .find(|(_, node)| ready(*node))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EVERY: Duration = Duration::from_secs(45);
    const HOLD: Duration = Duration::from_secs(15);
    const GAP: Duration = Duration::from_secs(30);

    fn all(_: NodeId) -> bool {
        true
    }

    #[test]
    fn rotates_through_nodes_on_schedule() {
        let t0 = Instant::now();
        let mut cycle = DiveCycle::new(EVERY, HOLD, t0);
        assert_eq!(cycle.tick(t0 + GAP - Duration::from_secs(1), all), None);
        assert_eq!(
            cycle.tick(t0 + GAP, all),
            Some(Change::Dived(NodeId::Zaibatsu))
        );
        assert_eq!(cycle.next_in(t0 + GAP), Some(HOLD));
        assert_eq!(cycle.tick(t0 + GAP + HOLD, all), Some(Change::Surfaced));
        let second = t0 + GAP + HOLD + GAP;
        assert_eq!(cycle.tick(second, all), Some(Change::Dived(NodeId::Atmos)));
    }

    #[test]
    fn skips_nodes_with_nothing_to_show() {
        let t0 = Instant::now();
        let mut cycle = DiveCycle::new(EVERY, HOLD, t0);
        let only_sky = |n| n == NodeId::Sky;
        assert_eq!(cycle.peek(only_sky), Some(NodeId::Sky));
        assert_eq!(
            cycle.tick(t0 + GAP, only_sky),
            Some(Change::Dived(NodeId::Sky))
        );
        // With nothing ready at all, it just waits another gap.
        let mut idle = DiveCycle::new(EVERY, HOLD, t0);
        assert_eq!(idle.tick(t0 + GAP, |_| false), None);
        assert_eq!(idle.next_in(t0 + GAP), Some(GAP));
    }

    #[test]
    fn priority_jumps_the_queue_and_comes_soon() {
        let t0 = Instant::now();
        let mut cycle = DiveCycle::new(EVERY, HOLD, t0);
        cycle.prioritize(NodeId::Seismic, t0);
        assert_eq!(cycle.next_in(t0), Some(PRIORITY_LEAD));
        assert_eq!(
            cycle.tick(t0 + PRIORITY_LEAD, all),
            Some(Change::Dived(NodeId::Seismic))
        );
        // The rotation carries on from where it was, not from the priority node.
        cycle.surface(t0 + PRIORITY_LEAD);
        let next = t0 + PRIORITY_LEAD + GAP;
        assert_eq!(cycle.tick(next, all), Some(Change::Dived(NodeId::Zaibatsu)));
    }

    #[test]
    fn priority_during_a_dive_goes_next_after_surfacing() {
        let t0 = Instant::now();
        let mut cycle = DiveCycle::new(EVERY, HOLD, t0);
        cycle.dive_now(NodeId::Atmos, t0);
        cycle.prioritize(NodeId::Helios, t0);
        assert_eq!(cycle.tick(t0 + HOLD, all), Some(Change::Surfaced));
        assert_eq!(cycle.next_in(t0 + HOLD), Some(PRIORITY_LEAD));
        let soon = t0 + HOLD + PRIORITY_LEAD;
        assert_eq!(cycle.tick(soon, all), Some(Change::Dived(NodeId::Helios)));
    }

    #[test]
    fn priority_on_the_current_dive_extends_it() {
        let t0 = Instant::now();
        let mut cycle = DiveCycle::new(EVERY, HOLD, t0);
        cycle.dive_now(NodeId::Seismic, t0);
        cycle.prioritize(NodeId::Seismic, t0 + Duration::from_secs(10));
        assert_eq!(cycle.next_in(t0 + Duration::from_secs(10)), Some(HOLD));
    }

    #[test]
    fn hold_freezes_everything_until_released() {
        let t0 = Instant::now();
        let mut cycle = DiveCycle::new(EVERY, HOLD, t0);
        cycle.dive_now(NodeId::Sky, t0);
        cycle.toggle_hold(t0);
        assert!(cycle.held());
        assert_eq!(cycle.next_in(t0), None);
        assert_eq!(cycle.tick(t0 + Duration::from_secs(600), all), None);
        assert_eq!(cycle.diving(), Some(NodeId::Sky));
        let later = t0 + Duration::from_secs(600);
        cycle.toggle_hold(later);
        assert_eq!(cycle.next_in(later), Some(HOLD));
    }

    #[test]
    fn space_bar_surfaces_or_dives() {
        let t0 = Instant::now();
        let mut cycle = DiveCycle::new(EVERY, HOLD, t0);
        assert_eq!(
            cycle.advance(t0, all),
            Some(Change::Dived(NodeId::Zaibatsu))
        );
        assert_eq!(cycle.advance(t0, all), Some(Change::Surfaced));
        assert_eq!(cycle.diving(), None);
        assert_eq!(cycle.advance(t0, |_| false), None);
    }
}
