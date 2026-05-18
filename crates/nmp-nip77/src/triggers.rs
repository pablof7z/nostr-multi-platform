//! Trigger fan-out for the three sync triggers documented in
//! `docs/plan/m4-negentropy.md`:
//!
//! 1. **app foreground** → reconcile every open `(filter, relay)` against its
//!    persisted watermark.
//! 2. **view open with gap** → reconcile the specific pair the freshly-opened
//!    view depends on.
//! 3. **relay reconnect** → reconcile every `(filter, _)` against the relay
//!    that just came back.
//!
//! ## Design
//!
//! The [`TriggerEngine`] owns the open-filter map and, for each trigger
//! event, produces a deduplicated work list ([`ReconcileWork`]).  The list
//! goes to the planner / actor for execution; the engine itself never opens
//! a socket or touches the store.  This keeps the trigger layer pure-data
//! and unit-testable without a relay harness.
//!
//! ## Doctrine
//!
//! * **D2** — every trigger results in a sync request before any REQ is
//!   considered.
//! * **D6** — the trigger surface is `Result`-free: failures (e.g. unknown
//!   filter hash) are silently dropped; observable consequences land as
//!   metrics, not exceptions.
//! * **D8** — the dedup pass keeps the work list bounded by the open-filter
//!   count, not by trigger throughput.

use std::collections::{BTreeMap, BTreeSet};

/// A trigger event the engine knows how to handle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TriggerEvent {
    /// App came back to the foreground.
    Foreground,
    /// A view was just opened with the given `(filter_hash, relay_url)`
    /// dependency.  Use this *only* when the view's coverage check already
    /// produced `PartialUpTo` / `Unknown`; views that hit `CompleteAsOf`
    /// should never reach this trigger.
    ViewOpenedWithGap {
        filter_hash: [u8; 32],
        relay_url: String,
    },
    /// A relay reconnected (e.g. network resumed, WebSocket reopened).
    RelayReconnected { relay_url: String },
}

/// One unit of reconciliation work emitted by the engine.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ReconcileWork {
    pub filter_hash: [u8; 32],
    pub relay_url: String,
}

/// In-memory book-keeping for the trigger engine.
pub struct TriggerEngine {
    /// `(relay_url, filter_hash)` pairs currently held open by views.  The
    /// `BTreeMap`/`BTreeSet` choice gives the engine deterministic output
    /// order — useful for tests and for diffing diagnostic snapshots.
    open: BTreeMap<String, BTreeSet<[u8; 32]>>,
}

impl TriggerEngine {
    pub fn new() -> Self {
        Self {
            open: BTreeMap::new(),
        }
    }

    /// Register an open `(filter, relay)` pair.  Calling [`register`] twice
    /// for the same pair is a no-op (set semantics).
    pub fn register(&mut self, filter_hash: [u8; 32], relay_url: impl Into<String>) {
        self.open
            .entry(relay_url.into())
            .or_default()
            .insert(filter_hash);
    }

    /// Drop a previously-registered pair.  Idempotent.
    pub fn unregister(&mut self, filter_hash: &[u8; 32], relay_url: &str) {
        if let Some(set) = self.open.get_mut(relay_url) {
            set.remove(filter_hash);
            if set.is_empty() {
                self.open.remove(relay_url);
            }
        }
    }

    /// True iff at least one pair is registered.
    pub fn is_empty(&self) -> bool {
        self.open.is_empty()
    }

    /// Map a trigger event to its reconciliation work list.  Output order is
    /// deterministic (relay-url-major, filter-hash-minor).
    pub fn on_event(&self, event: TriggerEvent) -> Vec<ReconcileWork> {
        let mut out: BTreeSet<ReconcileWork> = BTreeSet::new();
        match event {
            TriggerEvent::Foreground => {
                for (relay, filters) in &self.open {
                    for fh in filters {
                        out.insert(ReconcileWork {
                            filter_hash: *fh,
                            relay_url: relay.clone(),
                        });
                    }
                }
            }
            TriggerEvent::ViewOpenedWithGap {
                filter_hash,
                relay_url,
            } => {
                out.insert(ReconcileWork {
                    filter_hash,
                    relay_url,
                });
            }
            TriggerEvent::RelayReconnected { relay_url } => {
                if let Some(filters) = self.open.get(&relay_url) {
                    for fh in filters {
                        out.insert(ReconcileWork {
                            filter_hash: *fh,
                            relay_url: relay_url.clone(),
                        });
                    }
                }
            }
        }
        out.into_iter().collect()
    }
}

impl Default for TriggerEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fh(b: u8) -> [u8; 32] {
        [b; 32]
    }

    #[test]
    fn foreground_fans_out_over_all_registered_pairs() {
        let mut eng = TriggerEngine::new();
        eng.register(fh(1), "wss://a/");
        eng.register(fh(2), "wss://a/");
        eng.register(fh(1), "wss://b/");
        let work = eng.on_event(TriggerEvent::Foreground);
        assert_eq!(work.len(), 3);
        // Output is sorted by the derived `Ord` on `ReconcileWork`:
        // `filter_hash` first, then `relay_url`. That makes the order
        // [(fh1, a), (fh1, b), (fh2, a)] — deterministic, regardless of
        // insertion order.
        assert_eq!(work[0].filter_hash, fh(1));
        assert_eq!(work[0].relay_url, "wss://a/");
        assert_eq!(work[1].filter_hash, fh(1));
        assert_eq!(work[1].relay_url, "wss://b/");
        assert_eq!(work[2].filter_hash, fh(2));
        assert_eq!(work[2].relay_url, "wss://a/");
    }

    #[test]
    fn view_opened_emits_single_work_item() {
        let eng = TriggerEngine::new();
        let work = eng.on_event(TriggerEvent::ViewOpenedWithGap {
            filter_hash: fh(7),
            relay_url: "wss://x/".into(),
        });
        assert_eq!(work.len(), 1);
        assert_eq!(work[0].filter_hash, fh(7));
    }

    #[test]
    fn reconnect_fans_out_only_over_that_relay() {
        let mut eng = TriggerEngine::new();
        eng.register(fh(1), "wss://a/");
        eng.register(fh(2), "wss://a/");
        eng.register(fh(1), "wss://b/");
        let work = eng.on_event(TriggerEvent::RelayReconnected {
            relay_url: "wss://a/".into(),
        });
        assert_eq!(work.len(), 2);
        for w in &work {
            assert_eq!(w.relay_url, "wss://a/");
        }
    }

    #[test]
    fn reconnect_on_unknown_relay_produces_empty_work() {
        let mut eng = TriggerEngine::new();
        eng.register(fh(1), "wss://a/");
        let work = eng.on_event(TriggerEvent::RelayReconnected {
            relay_url: "wss://nobody/".into(),
        });
        assert!(work.is_empty());
    }

    #[test]
    fn register_unregister_round_trip() {
        let mut eng = TriggerEngine::new();
        eng.register(fh(1), "wss://a/");
        eng.unregister(&fh(1), "wss://a/");
        assert!(eng.is_empty());
        // Idempotent unregister.
        eng.unregister(&fh(1), "wss://a/");
    }

    #[test]
    fn registering_same_pair_twice_is_set_semantics() {
        let mut eng = TriggerEngine::new();
        eng.register(fh(1), "wss://a/");
        eng.register(fh(1), "wss://a/");
        let work = eng.on_event(TriggerEvent::Foreground);
        assert_eq!(work.len(), 1);
    }
}
