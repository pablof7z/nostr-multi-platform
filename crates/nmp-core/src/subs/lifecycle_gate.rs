//! Per-sub lifecycle tracking — OneShot CLOSE on EOSE, BoundedTime CLOSE on
//! deadline. Owns the `known_subs` map keyed by `(RelayUrl, sub_id)`.
//!
//! ## Gate key vs wire sub-id (mirrors subs/wire.rs reasoning)
//!
//! Wire sub-ids (`sub_id_for`) are derived from `canonical_filter_hash`
//! alone, not the relay URL. Per NIP-01 §1, subscription ids are
//! per-connection — the same filter on two relay connections may legitimately
//! share the same sub-id string. The gate's internal key is therefore
//! `(RelayUrl, sub_id)`, NOT `sub_id` alone: two relays carrying the same
//! filter hash are distinct subscriptions from the gate's perspective. Without
//! relay-scoped keying, the second `observe_diff` REQ silently overwrites the
//! first, causing EOSE / deadline CLOSEs to miss the overwritten relay's sub
//! (relay leaks). (#166, follows #161 `wire.rs` fix.)
//!
//! Pure data structure; the lifecycle controller in `mod.rs` decides when to
//! call the methods here based on incoming relay frames.

use std::collections::HashMap;

use super::wire::WireFrame;
use crate::planner::{InterestId, InterestLifecycle, RelayUrl};

/// Per-wire-sub bookkeeping for lifecycle decisions.
#[derive(Clone, Debug)]
pub(super) struct KnownSub {
    pub(super) lifecycle: InterestLifecycle,
    /// The originating logical-interest id. Held so future view-module
    /// integration can correlate wire-sub → consumer when EOSE / CLOSED
    /// frames arrive.
    #[allow(dead_code)]
    pub(super) interest_id: InterestId,
}

/// Tracks which wire subs are currently open and how each should close.
///
/// Keyed by `(relay_url, sub_id)` — relay-scoped so that two relays carrying
/// the same filter hash (same sub_id string) are tracked independently.
/// See module doc for the NIP-01 rationale.
#[derive(Default)]
pub(super) struct LifecycleGate {
    known_subs: HashMap<(RelayUrl, String), KnownSub>,
}

impl LifecycleGate {
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// Reconcile the bookkeeping with a freshly-computed diff. REQs are added
    /// to the known set; CLOSEs remove them.
    ///
    /// Both insert and remove use `(relay_url, sub_id)` as the key — the
    /// relay-scoped gate key (not the wire sub-id alone). See module doc.
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
                        (relay_url.clone(), sub_id.clone()),
                        KnownSub {
                            lifecycle: lifecycle.clone(),
                            interest_id: interest_id.clone(),
                        },
                    );
                }
                WireFrame::Close { relay_url, sub_id } => {
                    self.known_subs.remove(&(relay_url.clone(), sub_id.clone()));
                }
            }
        }
    }

    /// EOSE → CLOSE for OneShot subs; no-op otherwise.
    ///
    /// Lookup uses `(relay_url, sub_id)` — the relay-scoped key — so EOSE on
    /// relay A only affects relay A's entry, even when relay B shares the
    /// same sub_id string. The relay-mismatch guard from the old single-key
    /// scheme is not needed here: if the key is absent, the sub is unknown.
    pub(super) fn on_eose(&mut self, relay_url: &str, sub_id: &str) -> Vec<WireFrame> {
        let key = (relay_url.to_string(), sub_id.to_string());
        let Some(sub) = self.known_subs.get(&key).cloned() else {
            return Vec::new();
        };
        match sub.lifecycle {
            InterestLifecycle::OneShot => {
                self.known_subs.remove(&key);
                vec![WireFrame::Close {
                    relay_url: relay_url.to_string(),
                    sub_id: sub_id.to_string(),
                }]
            }
            InterestLifecycle::Tailing | InterestLifecycle::BoundedTime { .. } => Vec::new(),
        }
    }

    /// Tick deadlines: CLOSE every BoundedTime sub whose `until_ms` has passed.
    ///
    /// Iterates `(relay_url, sub_id)` pairs so two relays sharing a filter
    /// hash (same sub_id) each produce an independent CLOSE.
    pub(super) fn tick_deadlines(&mut self, now_ms: u64) -> Vec<WireFrame> {
        let expired: Vec<(RelayUrl, String)> = self
            .known_subs
            .iter()
            .filter_map(|((relay_url, sub_id), sub)| match sub.lifecycle {
                InterestLifecycle::BoundedTime { until_ms } if now_ms >= until_ms => {
                    Some((relay_url.clone(), sub_id.clone()))
                }
                _ => None,
            })
            .collect();
        let mut closes = Vec::with_capacity(expired.len());
        for (relay_url, sub_id) in expired {
            self.known_subs.remove(&(relay_url.clone(), sub_id.clone()));
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

    // ─── RED: cross-relay shared-filter collision (#166) ─────────────────────

    /// OneShot on two relays with the SAME filter hash (same sub_id) — EOSE on
    /// relay A must emit exactly one CLOSE for relay A, and the relay-B sub
    /// must remain known so its own EOSE later still fires a CLOSE.
    ///
    /// With the buggy `HashMap<sub_id>` keying, the second REQ (relay B)
    /// overwrites the first (relay A), so EOSE on relay A hits the relay
    /// mismatch guard and emits nothing — relay A's sub leaks on the wire.
    #[test]
    fn oneshot_cross_relay_shared_filter_each_eose_closes_own_relay() {
        let mut g = LifecycleGate::new();
        // Same sub_id on two relays (identical filter hash — the realistic
        // scenario when a OneShot interest routes to multiple relays).
        g.observe_diff(&[
            req("shared-filter", "wss://relay-a", InterestLifecycle::OneShot),
            req("shared-filter", "wss://relay-b", InterestLifecycle::OneShot),
        ]);

        // EOSE on relay A → must produce a CLOSE for relay A only.
        let closes_a = g.on_eose("wss://relay-a", "shared-filter");
        assert_eq!(
            closes_a.len(),
            1,
            "EOSE on relay-a must emit exactly 1 CLOSE; got {closes_a:?}"
        );
        assert!(
            matches!(&closes_a[0], WireFrame::Close { relay_url, sub_id }
                if relay_url == "wss://relay-a" && sub_id == "shared-filter"),
            "CLOSE must target relay-a; got {:?}",
            closes_a
        );

        // Relay B's sub must still be known → EOSE on relay B must also close.
        let closes_b = g.on_eose("wss://relay-b", "shared-filter");
        assert_eq!(
            closes_b.len(),
            1,
            "EOSE on relay-b must emit exactly 1 CLOSE; got {closes_b:?}"
        );
        assert!(
            matches!(&closes_b[0], WireFrame::Close { relay_url, sub_id }
                if relay_url == "wss://relay-b" && sub_id == "shared-filter"),
            "CLOSE must target relay-b; got {:?}",
            closes_b
        );
    }

    /// BoundedTime on two relays with the SAME filter hash — deadline tick must
    /// emit two CLOSE frames, one per relay.
    ///
    /// With single-key keying, only one entry exists in `known_subs`, so
    /// `tick_deadlines` produces at most one CLOSE; the other relay's sub
    /// silently leaks past its deadline.
    #[test]
    fn bounded_time_cross_relay_shared_filter_deadline_closes_both_relays() {
        let mut g = LifecycleGate::new();
        g.observe_diff(&[
            req(
                "shared-filter",
                "wss://relay-a",
                InterestLifecycle::BoundedTime { until_ms: 100 },
            ),
            req(
                "shared-filter",
                "wss://relay-b",
                InterestLifecycle::BoundedTime { until_ms: 100 },
            ),
        ]);

        let closes = g.tick_deadlines(101);
        assert_eq!(
            closes.len(),
            2,
            "deadline must close both relay subs; got {closes:?}"
        );
        let relay_urls: std::collections::BTreeSet<&str> = closes
            .iter()
            .filter_map(|f| match f {
                WireFrame::Close { relay_url, .. } => Some(relay_url.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            relay_urls.contains("wss://relay-a"),
            "relay-a must receive a CLOSE; got {closes:?}"
        );
        assert!(
            relay_urls.contains("wss://relay-b"),
            "relay-b must receive a CLOSE; got {closes:?}"
        );
    }
}
