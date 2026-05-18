//! Per-sub lifecycle tracking — OneShot CLOSE on EOSE, BoundedTime CLOSE on
//! deadline. Owns the `known_subs` map keyed by wire sub-id.
//!
//! Pure data structure; the lifecycle controller in `mod.rs` decides when to
//! call the methods here based on incoming relay frames.

use std::collections::HashMap;

use super::wire::WireFrame;
use crate::planner::{InterestId, InterestLifecycle, RelayUrl};

/// Per-wire-sub bookkeeping for lifecycle decisions.
#[derive(Clone, Debug)]
pub(super) struct KnownSub {
    pub(super) relay_url: RelayUrl,
    pub(super) lifecycle: InterestLifecycle,
    /// The originating logical-interest id. Held so future view-module
    /// integration can correlate wire-sub → consumer when EOSE / CLOSED
    /// frames arrive.
    #[allow(dead_code)]
    pub(super) interest_id: InterestId,
}

/// Tracks which wire subs are currently open and how each should close.
#[derive(Default)]
pub(super) struct LifecycleGate {
    known_subs: HashMap<String, KnownSub>,
}

impl LifecycleGate {
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// Reconcile the bookkeeping with a freshly-computed diff. REQs are added
    /// to the known set; CLOSEs remove them.
    pub(super) fn observe_diff(&mut self, frames: &[WireFrame]) {
        for frame in frames {
            match frame {
                WireFrame::Req {
                    sub_id,
                    relay_url,
                    interest_id,
                    lifecycle,
                    ..
                } => {
                    self.known_subs.insert(
                        sub_id.clone(),
                        KnownSub {
                            relay_url: relay_url.clone(),
                            lifecycle: lifecycle.clone(),
                            interest_id: interest_id.clone(),
                        },
                    );
                }
                WireFrame::Close { sub_id, .. } => {
                    self.known_subs.remove(sub_id);
                }
            }
        }
    }

    /// EOSE → CLOSE for OneShot subs; no-op otherwise.
    pub(super) fn on_eose(&mut self, relay_url: &str, sub_id: &str) -> Vec<WireFrame> {
        let Some(sub) = self.known_subs.get(sub_id).cloned() else {
            return Vec::new();
        };
        if sub.relay_url != relay_url {
            return Vec::new();
        }
        match sub.lifecycle {
            InterestLifecycle::OneShot => {
                self.known_subs.remove(sub_id);
                vec![WireFrame::Close {
                    relay_url: sub.relay_url,
                    sub_id: sub_id.to_string(),
                }]
            }
            InterestLifecycle::Tailing | InterestLifecycle::BoundedTime { .. } => Vec::new(),
        }
    }

    /// Tick deadlines: CLOSE every BoundedTime sub whose `until_ms` has passed.
    pub(super) fn tick_deadlines(&mut self, now_ms: u64) -> Vec<WireFrame> {
        let expired: Vec<(String, RelayUrl)> = self
            .known_subs
            .iter()
            .filter_map(|(sub_id, sub)| match sub.lifecycle {
                InterestLifecycle::BoundedTime { until_ms } if now_ms >= until_ms => {
                    Some((sub_id.clone(), sub.relay_url.clone()))
                }
                _ => None,
            })
            .collect();
        let mut closes = Vec::with_capacity(expired.len());
        for (sub_id, relay_url) in expired {
            self.known_subs.remove(&sub_id);
            closes.push(WireFrame::Close { relay_url, sub_id });
        }
        closes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(sub_id: &str, relay: &str, lc: InterestLifecycle) -> WireFrame {
        WireFrame::Req {
            relay_url: relay.to_string(),
            sub_id: sub_id.to_string(),
            filter_json: "{}".to_string(),
            interest_id: InterestId(0),
            lifecycle: lc,
        }
    }

    #[test]
    fn eose_closes_oneshot() {
        let mut g = LifecycleGate::new();
        g.observe_diff(&[req("s1", "wss://r", InterestLifecycle::OneShot)]);
        let closes = g.on_eose("wss://r", "s1");
        assert_eq!(closes.len(), 1);
    }

    #[test]
    fn eose_does_not_close_tailing() {
        let mut g = LifecycleGate::new();
        g.observe_diff(&[req("s1", "wss://r", InterestLifecycle::Tailing)]);
        let closes = g.on_eose("wss://r", "s1");
        assert!(closes.is_empty());
    }

    #[test]
    fn deadline_closes_bounded_time() {
        let mut g = LifecycleGate::new();
        g.observe_diff(&[req(
            "s1",
            "wss://r",
            InterestLifecycle::BoundedTime { until_ms: 100 },
        )]);
        let closes = g.tick_deadlines(50);
        assert!(closes.is_empty(), "deadline not yet reached");
        let closes = g.tick_deadlines(101);
        assert_eq!(closes.len(), 1, "deadline passed → CLOSE");
    }

    #[test]
    fn eose_on_unknown_sub_is_noop() {
        let mut g = LifecycleGate::new();
        assert!(g.on_eose("wss://r", "ghost").is_empty());
    }
}
