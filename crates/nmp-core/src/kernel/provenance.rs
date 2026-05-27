//! Per-relay first-source provenance accounting (T131; F2 dep of
//! `docs/design/outbox-explorer-diagnostics.md`).
//!
//! Tracks **which relay was the first source** of every event id the kernel
//! has ever ingested, and surfaces per-URL `(novel, duplicate, replaced,
//! rejected)` counters for the diagnostic projection (`RelayUsefulness`,
//! design doc §2 line 152). The store's `InsertOutcome` discriminant is the
//! authoritative signal — `Inserted | Replaced` ⇒ first source for that id,
//! `Duplicate` ⇒ a redundant copy on a different socket.
//!
//! ## D8 — bounds and hot-path cost
//!
//! No per-event allocation on the ingest hot path. Counter writes are
//! `u64::saturating_add` on owned per-URL bags (single-writer under D4 —
//! no atomics needed). The bag is keyed by relay URL, so the working set
//! is `O(active_sockets)` — capped by the kernel's own URL pool (the §1
//! single-socket invariant of the design doc).
//!
//! Crucially **no event-id → relay map is maintained here**: the store
//! already owns the bounded `ProvenanceEntry` sidecar (`store/mem/mod.rs`
//! `MAX_PROVENANCE_ENTRIES`-bounded, `primary: bool` set on the oldest by
//! `first_seen_ms`). `first_source(id)` is therefore a delegation to
//! `store.provenance_for(id)` looking for `primary == true` — no
//! duplicated state, no kernel-side LRU (the design doc §3 forbids the
//! latter as a D8 violation).
//!
//! ## D0 — kernel vocabulary only
//!
//! No app nouns; works on any event id and any relay URL. All consumers
//! live outside the kernel (design doc §6).

use std::collections::HashMap;

use crate::store::{EventStore, ProvenanceEntry, RelayUrl, StoreError};

/// Per-URL usefulness counters mirroring `RelayUsefulness` in the design
/// doc §2 (lines 152–168). Widths are saturating `u64` — overflow is a
/// numeric artefact, never a wrap (design doc §2 closing note).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::kernel) struct RelayCounters {
    /// `InsertOutcome::Inserted` — this socket was the first to deliver
    /// the event id.
    pub novel: u64,
    /// `InsertOutcome::Duplicate` — this socket re-delivered an event
    /// already first-sourced by another URL (or by itself on a prior tick).
    pub duplicate: u64,
    /// `InsertOutcome::Replaced` — this socket delivered a newer
    /// replaceable that won supersession.
    pub replaced: u64,
    /// `InsertOutcome::Rejected` — sig/delegation/structural fail on this
    /// socket's frame.
    pub rejected: u64,
}

/// Kernel-owned per-URL provenance bag. Single-writer (D4); held inline on
/// `Kernel`. The map grows with the active socket population, which the
/// transport pool itself caps (§1 of the design doc). When a URL is
/// LRU-evicted from the URL-health table (F3, future task), its row here
/// is dropped in the same sweep.
#[derive(Clone, Debug, Default)]
pub(in crate::kernel) struct EventProvenance {
    counters: HashMap<RelayUrl, RelayCounters>,
}

impl EventProvenance {
    pub(in crate::kernel) fn new() -> Self {
        Self::default()
    }

    /// Record an `Inserted` outcome for `relay_url`. `_event_id` is not
    /// indexed (the store already owns the event-id sidecar via
    /// `ProvenanceEntry`); the parameter is kept for call-site documentation
    /// and for future tracing hooks.
    pub(in crate::kernel) fn record_first_source(&mut self, _event_id: &str, relay_url: &str) {
        let slot = self.entry_mut(relay_url);
        slot.novel = slot.novel.saturating_add(1);
    }

    /// Record an `Replaced` outcome — this URL delivered a newer
    /// replaceable. Counted separately from `novel` so the diagnostic UI
    /// can render supersession churn distinctly from cold first-sourcing.
    pub(in crate::kernel) fn record_replaced(&mut self, relay_url: &str) {
        let slot = self.entry_mut(relay_url);
        slot.replaced = slot.replaced.saturating_add(1);
    }

    /// Record a `Duplicate` outcome — this URL delivered an id another URL
    /// (or this one on a prior tick) already first-sourced.
    pub(in crate::kernel) fn record_duplicate(&mut self, relay_url: &str) {
        let slot = self.entry_mut(relay_url);
        slot.duplicate = slot.duplicate.saturating_add(1);
    }

    /// Record a `Rejected` outcome — sig-fail / NIP-40 expired / malformed.
    pub(in crate::kernel) fn record_rejected(&mut self, relay_url: &str) {
        let slot = self.entry_mut(relay_url);
        slot.rejected = slot.rejected.saturating_add(1);
    }

    /// Read-only snapshot of one URL's counters. `None` if the URL has
    /// never been credited or charged for an outcome.
    #[allow(dead_code)] // F4 `relay_diagnostics()` projection + tests
    pub(in crate::kernel) fn counters_for(&self, relay_url: &str) -> Option<RelayCounters> {
        self.counters.get(relay_url).copied()
    }

    /// Iterator over `(url, counters)` for the snapshot projection. The
    /// caller is `kernel::status::relay_diagnostics` (F4, future task).
    #[allow(dead_code)] // exposed for F4 `relay_diagnostics()` projection
    pub(in crate::kernel) fn iter(&self) -> impl Iterator<Item = (&RelayUrl, &RelayCounters)> {
        self.counters.iter()
    }

    /// Drop the per-URL row when the transport pool retires a socket
    /// (F3 evicts the URL-health row at ≥5 min idle; same sweep frees
    /// this entry — design doc §3 working-set bound).
    #[allow(dead_code)] // wired in F3 `relay_url_health` LRU eviction sweep
    pub(in crate::kernel) fn forget_url(&mut self, relay_url: &str) {
        self.counters.remove(relay_url);
    }

    /// Number of URLs currently tracked. Diagnostic only.
    #[cfg(test)]
    pub(in crate::kernel) fn url_count(&self) -> usize {
        self.counters.len()
    }

    fn entry_mut(&mut self, relay_url: &str) -> &mut RelayCounters {
        // D8-aware: the steady-state alloc concern is the `to_string()`
        // on the `entry()` key. We hit-check first via `contains_key`
        // to keep the hit path alloc-free; the miss path pays one
        // `to_string()` per first-touched URL — bounded by the active
        // socket pool (design doc §1 single-socket-per-URL invariant,
        // typically tens, not thousands). D6 (no unwrap/expect): the
        // returning `or_insert_with` is panic-free; after the explicit
        // insert above, `entry()` lands on the existing slot without
        // running the closure.
        if !self.counters.contains_key(relay_url) {
            self.counters
                .insert(relay_url.to_string(), RelayCounters::default());
        }
        self.counters.entry(relay_url.to_string()).or_default()
    }
}

/// First-source query — delegates to the store's `provenance_for` sidecar,
/// which already maintains a bounded `ProvenanceEntry` list per id with
/// `primary: bool` set on the entry with the smallest `first_seen_ms`
/// (`store/mem/mod.rs::sort_provenance`). Returns the URL of the primary
/// entry, or `None` if the id is not in the store (or has no provenance).
///
/// D8: no kernel-side index of event-id → URL. The store is the
/// single source of truth (D4). This function is a thin read-side
/// projection; cost is one `provenance_for` call (bounded by
/// `MAX_PROVENANCE_ENTRIES`, currently small).
#[allow(dead_code)] // F4 explorer projection consumes this; kept here as the
                    // single seam between `EventProvenance` counters and the
                    // store's per-id sidecar.
pub(in crate::kernel) fn first_source_for(
    store: &dyn EventStore,
    event_id_bytes: &[u8; 32],
) -> Result<Option<RelayUrl>, StoreError> {
    let entries = store.provenance_for(event_id_bytes)?;
    Ok(primary_url(&entries))
}

#[allow(dead_code)] // internal helper for `first_source_for`; tested directly.
fn primary_url(entries: &[ProvenanceEntry]) -> Option<RelayUrl> {
    entries
        .iter()
        .find(|e| e.primary)
        .map(|e| e.relay_url.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_first_source_increments_novel_only() {
        let mut p = EventProvenance::new();
        p.record_first_source("evt-1", "wss://a.example");
        let c = p.counters_for("wss://a.example").expect("URL inserted");
        assert_eq!(c.novel, 1);
        assert_eq!(c.duplicate, 0);
        assert_eq!(c.replaced, 0);
        assert_eq!(c.rejected, 0);
    }

    #[test]
    fn record_duplicate_credits_second_relay_only() {
        // First-source determinism: relay A delivers first → novel++;
        // relay B re-delivers same id → B.duplicate++; A's counters
        // untouched.
        let mut p = EventProvenance::new();
        p.record_first_source("evt-1", "wss://a.example");
        p.record_duplicate("wss://b.example");

        let a = p.counters_for("wss://a.example").expect("a counted");
        let b = p.counters_for("wss://b.example").expect("b counted");
        assert_eq!(a.novel, 1, "first source A keeps its novel credit");
        assert_eq!(a.duplicate, 0, "A is NOT charged for B's redelivery");
        assert_eq!(b.duplicate, 1, "B is charged with one duplicate");
        assert_eq!(b.novel, 0, "B was not the first source");
    }

    #[test]
    fn multi_relay_novelty_ratio_70_30_split() {
        // 100 events, 70 first-sourced by A and 30 by B, no overlap.
        // Each relay's novel_count must match its share exactly.
        let mut p = EventProvenance::new();
        for i in 0..70 {
            p.record_first_source(&format!("evt-{i}"), "wss://a.example");
        }
        for i in 70..100 {
            p.record_first_source(&format!("evt-{i}"), "wss://b.example");
        }

        let a = p.counters_for("wss://a.example").expect("a counted");
        let b = p.counters_for("wss://b.example").expect("b counted");
        assert_eq!(a.novel, 70);
        assert_eq!(b.novel, 30);
        // Snapshot-side ratio computation (design doc §2 line 161 —
        // `novelty_ratio: Option<f32>` is derived at projection time,
        // not stored here):
        let a_ratio = a.novel as f32 / (a.novel.saturating_add(a.duplicate) as f32);
        let b_ratio = b.novel as f32 / (b.novel.saturating_add(b.duplicate) as f32);
        assert!((a_ratio - 1.0).abs() < f32::EPSILON, "no dups → 1.0");
        assert!((b_ratio - 1.0).abs() < f32::EPSILON, "no dups → 1.0");
        assert_eq!(p.url_count(), 2);
    }

    #[test]
    fn replaced_and_rejected_counted_separately() {
        let mut p = EventProvenance::new();
        p.record_replaced("wss://a.example");
        p.record_replaced("wss://a.example");
        p.record_rejected("wss://a.example");

        let a = p.counters_for("wss://a.example").expect("a counted");
        assert_eq!(a.replaced, 2);
        assert_eq!(a.rejected, 1);
        assert_eq!(a.novel, 0);
        assert_eq!(a.duplicate, 0);
    }

    #[test]
    fn saturating_counters_never_wrap() {
        // D8 invariant: counter widths are saturating, never wrap.
        let mut p = EventProvenance::new();
        p.counters.insert(
            "wss://a.example".to_string(),
            RelayCounters {
                novel: u64::MAX,
                duplicate: 0,
                replaced: 0,
                rejected: 0,
            },
        );
        p.record_first_source("evt-X", "wss://a.example");
        let a = p.counters_for("wss://a.example").expect("a counted");
        assert_eq!(a.novel, u64::MAX, "saturating_add holds at u64::MAX");
    }

    #[test]
    fn forget_url_drops_the_row() {
        let mut p = EventProvenance::new();
        p.record_first_source("evt-1", "wss://a.example");
        p.record_first_source("evt-2", "wss://b.example");
        assert_eq!(p.url_count(), 2);
        p.forget_url("wss://a.example");
        assert_eq!(p.url_count(), 1);
        assert!(p.counters_for("wss://a.example").is_none());
        assert!(p.counters_for("wss://b.example").is_some());
    }

    #[test]
    fn primary_url_picks_the_flagged_entry() {
        // Unit test for the `first_source_for` helper's pure inner: the
        // primary-flag filter. Verifies the convention that the store
        // sets `primary: true` on exactly one entry per id.
        let entries = vec![
            ProvenanceEntry {
                relay_url: "wss://b.example".to_string(),
                first_seen_ms: 200,
                last_seen_ms: 200,
                primary: false,
            },
            ProvenanceEntry {
                relay_url: "wss://a.example".to_string(),
                first_seen_ms: 100,
                last_seen_ms: 100,
                primary: true,
            },
        ];
        assert_eq!(
            primary_url(&entries),
            Some("wss://a.example".to_string()),
            "the primary-flagged entry's URL is returned"
        );
    }

    #[test]
    fn primary_url_none_when_no_entries() {
        assert_eq!(primary_url(&[]), None);
    }
}
