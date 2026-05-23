//! `ZapsAggregateProjection` — the read-side of a multi-target zap-count
//! display (timeline rows, post cards, etc.).
//!
//! This is **pure consumption**: a [`KernelEventObserver`] that decodes incoming
//! kind:9735 zap receipts and aggregates them, keyed by the receipt's
//! `["e", zapped_event_id]` tag. The shell asks "how many sats has note X
//! received and from how many distinct receipts?" — exactly the per-row
//! affordance a feed surface needs.
//!
//! It registers no actions, mints no FFI symbols, and never touches the actor
//! loop.
//!
//! ## Why a separate projection from [`ZapsView`]
//!
//! [`ZapsView`] is single-target: it answers "show me everyone who zapped this
//! one note" — opened on demand when a detail screen wants the zapper list.
//! `ZapsAggregateProjection` is multi-target: it answers "for every note that
//! has been zapped, what is the running total?" — the affordance a *timeline*
//! needs, where every visible row wants its own count. The two are
//! complementary; the aggregate uses receipt counts and msat sums, not the
//! per-zapper detail [`ZapsView`] carries.
//!
//! ## How it plugs into the snapshot seam
//!
//! NMP has two complementary extension seams (see
//! `nmp_core::kernel::snapshot_registry` and
//! `nmp_core::actor::commands::event_observer`):
//!
//! - **`KernelEventObserver`** — the *ingest* side. `on_kernel_event` fires
//!   once per accepted event on the actor thread; the projection accumulates
//!   the facts it cares about into its own interior-mutable state.
//! - **`register_snapshot_projection`** — the *output* side. A host registers
//!   a no-argument closure that runs on every snapshot tick and returns a
//!   `serde_json::Value` appended under a host-chosen key.
//!
//! `ZapsAggregateProjection` is built to sit on *both*: it implements
//! `KernelEventObserver` for ingest, and exposes
//! [`ZapsAggregateProjection::snapshot_json`] — a cheap, non-blocking,
//! no-argument read — so the host can register it as
//!
//! ```ignore
//! let projection = Arc::new(ZapsAggregateProjection::new());
//! let observer_id = app.register_event_observer(
//!     Arc::clone(&projection) as Arc<dyn KernelEventObserver>,
//! );
//! let snap = Arc::clone(&projection);
//! app.register_snapshot_projection("nmp.nip57.zaps", move || snap.snapshot_json());
//! ```
//!
//! Wiring that closure is the host app composition crate's job (a separate
//! PR); this crate only provides the projection type and a snapshot read
//! that is safe to call from inside a tick.
//!
//! ## D8 — non-blocking
//!
//! [`ZapsAggregateProjection::snapshot_json`] runs on the actor thread inside
//! the snapshot tick. It takes one uncontended `Mutex` lock and clones a small
//! map — no I/O, no relay round-trips, no event-store scan. The accumulation
//! work (`on_kernel_event`) is likewise a single lock + map insert.
//!
//! ## What this projection does and does not index
//!
//! Only kind:9735 receipts that carry an `["e", target_event_id]` tag are
//! aggregated; the indexing key is that target event id. Receipts that aim at
//! a profile (`p`-only) or an addressable coordinate (`a`-tag) are ignored —
//! those need a sibling `nmp.nip57.zaps_by_profile` / `_by_address` projection,
//! intentionally out of scope here. This matches the precedent set by the
//! reverse-index `domain::decode_and_route` (`domain.rs`).
//!
//! Subscribing to kind:9735 events targeted at a given viewer is a relay
//! routing concern handled at registration time via [`ViewDependencies`] /
//! `LogicalInterest`, not inside the observer. The projection trusts that the
//! upstream subscription delivers receipts; it has nothing to filter on at the
//! observer level beyond "kind:9735 + has `e` tag", both of which the decoder
//! already enforces.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use nmp_core::substrate::{
    BoundedMessageMap, EventId, KernelEvent, MAX_PROJECTION_MESSAGES,
};
use nmp_core::KernelEventObserver;
use serde::{Deserialize, Serialize};

use crate::decode::try_from_kernel_event;

/// Aggregate zap totals for a single target event.
///
/// `total_msats` sums the authoritative bolt11 amount of every distinct
/// receipt indexed under this target; `count` is the number of distinct
/// receipts. A receipt whose amount could not be parsed (neither a bolt11 HRP
/// nor an embedded `amount` tag) contributes `0` msats but still increments
/// `count` — the zap *happened*, the amount is just unknown. This matches the
/// existing [`ZapsView`](crate::ZapsView) semantics.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct ZapCount {
    pub total_msats: u64,
    pub count: u32,
}

/// The serialised read-model a timeline-zap-count surface consumes.
///
/// `totals` maps a zapped event id to its running `ZapCount`. The wrapper
/// struct (rather than a bare map at the top level) is intentional and
/// mirrors how [`GroupChatSnapshot`](super) and `ModularTimelineSnapshot`
/// shape their snapshots — a named field is friendlier to deserialize on the
/// Swift / Kotlin side and leaves room to add sibling fields later without a
/// breaking re-shape.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct ZapsAggregateSnapshot {
    pub totals: HashMap<EventId, ZapCount>,
}

impl ZapsAggregateSnapshot {
    /// An empty snapshot — what a freshly-constructed projection (or a
    /// poisoned internal mutex, D6) reports.
    #[must_use] 
    pub fn empty() -> Self {
        Self {
            totals: HashMap::new(),
        }
    }
}

/// Accumulates kind:9735 zap-receipt amounts grouped by their zapped event id.
///
/// Construct with `new()` (the projection takes no spec — it indexes every
/// kind:9735 it sees, the subscription scoping is a relay-routing concern at
/// registration time). Register the same `Arc` as a [`KernelEventObserver`]
/// (ingest) and capture it in a snapshot-projection closure (output).
///
/// Internally the per-target state is `BTreeMap<EventId, u64>` (`receipt_id` →
/// msats), which gives free dedupe on `receipt_id` (the same receipt being
/// re-delivered across relays does not double-count) and a deterministic
/// iteration order for any future receipt-level introspection.
pub struct ZapsAggregateProjection {
    /// `target_event_id → (receipt_id → msats)`. The inner map's key dedupes
    /// re-deliveries of the same receipt; the value lets the snapshot derive
    /// both `count` (inner-map len) and `total_msats` (inner-map sum) on
    /// read.
    ///
    /// The outer map is bounded by [`MAX_PROJECTION_MESSAGES`]: once a busy
    /// session has been zapped on more than that many distinct target events,
    /// the oldest-by-first-receipt target is evicted to make room. The inner
    /// `BTreeMap` (per-receipt dedupe) is naturally bounded by the count of
    /// distinct zappers on one target — not separately capped because that
    /// dimension does not grow unboundedly the way "all targets ever seen"
    /// does.
    by_target: Mutex<BoundedMessageMap<EventId, BTreeMap<EventId, u64>>>,
}

impl Default for ZapsAggregateProjection {
    fn default() -> Self {
        Self::new()
    }
}

impl ZapsAggregateProjection {
    /// Construct an empty projection. Events arrive via
    /// [`KernelEventObserver::on_kernel_event`].
    #[must_use] 
    pub fn new() -> Self {
        Self {
            by_target: Mutex::new(BoundedMessageMap::new(MAX_PROJECTION_MESSAGES)),
        }
    }

    /// Snapshot the current zap totals as a typed [`ZapsAggregateSnapshot`].
    ///
    /// D6: a poisoned mutex degrades to [`ZapsAggregateSnapshot::empty`]
    /// rather than panicking — this can run on the actor thread inside a
    /// snapshot tick, where a panic would unwind the kernel.
    pub fn snapshot(&self) -> ZapsAggregateSnapshot {
        let Ok(by_target) = self.by_target.lock() else {
            return ZapsAggregateSnapshot::empty();
        };
        let totals: HashMap<EventId, ZapCount> = by_target
            .iter()
            .map(|(target, receipts)| {
                let count = u32::try_from(receipts.len()).unwrap_or(u32::MAX);
                let total_msats = receipts.values().sum();
                (
                    target.clone(),
                    ZapCount {
                        total_msats,
                        count,
                    },
                )
            })
            .collect();
        ZapsAggregateSnapshot { totals }
    }

    /// Snapshot as a `serde_json::Value` — the exact shape a host
    /// `register_snapshot_projection` closure must return.
    ///
    /// D6: a serialisation failure (not expected for this plain struct)
    /// collapses to `json!({"totals": {}})` rather than propagating.
    pub fn snapshot_json(&self) -> serde_json::Value {
        serde_json::to_value(self.snapshot())
            .unwrap_or_else(|_| serde_json::json!({ "totals": {} }))
    }
}

impl KernelEventObserver for ZapsAggregateProjection {
    /// Ingest one accepted kernel event. Non-receipts (wrong kind) and
    /// receipts that lack an `["e", target]` tag (profile / addressable
    /// zaps) are silently ignored — the decoder enforces both checks.
    /// Receipts with a parseable `e` tag accumulate under that target; a
    /// re-delivery of the same `receipt_id` replaces rather than duplicates
    /// (`BTreeMap` key dedupe).
    ///
    /// Cheap and panic-free, per the `KernelEventObserver` contract: one
    /// decode + one uncontended lock + two map inserts. A poisoned mutex is
    /// a silent no-op (D6).
    fn on_kernel_event(&self, event: &KernelEvent) {
        let Some(record) = try_from_kernel_event(event) else {
            return;
        };
        let Some(target) = record.zapped_event_id else {
            // Profile zap / addressable zap — out of scope for this
            // projection. See module docs.
            return;
        };
        let msats = record.amount_msats.unwrap_or(0);
        let Ok(mut by_target) = self.by_target.lock() else {
            return;
        };
        by_target
            .entry_or_insert_with(target, BTreeMap::new)
            .insert(record.event_id, msats);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Build a kind:9735 receipt with an explicit target `e` tag, sender, and
    /// a bolt11 stub whose HRP encodes `msats` (matches the `bolt11::amount_msats`
    /// helper conventions used elsewhere in the crate's tests).
    fn receipt(id: &str, target: &str, msats: u64, sender: Option<&str>) -> KernelEvent {
        let mut tags = vec![
            vec!["p".into(), "recipient".into()],
            vec!["e".into(), target.into()],
            // `lnbc<n>n…` — `n` is the nano-BTC suffix; `amount_msats` reads
            // the integer prefix and scales. We use the same shape the `view`
            // tests use so the decoded amount equals `msats`.
            vec!["bolt11".into(), format!("lnbc{}n1pvj...", msats / 100)],
        ];
        if let Some(s) = sender {
            tags.push(vec!["P".into(), s.into()]);
        }
        KernelEvent {
            id: id.into(),
            author: "ln_node".into(),
            kind: 9735,
            created_at: 1,
            tags,
            content: String::new(),
        }
    }

    #[test]
    fn fresh_projection_yields_empty_snapshot() {
        let proj = ZapsAggregateProjection::new();
        assert_eq!(proj.snapshot(), ZapsAggregateSnapshot::empty());
        let json = proj.snapshot_json();
        assert_eq!(json, serde_json::json!({ "totals": {} }));
    }

    #[test]
    fn one_receipt_is_indexed_under_its_target() {
        let proj = ZapsAggregateProjection::new();
        proj.on_kernel_event(&receipt("Z1", "NOTE", 15_000, Some("alice")));

        let snap = proj.snapshot();
        let count = snap
            .totals
            .get("NOTE")
            .expect("NOTE must be present after one receipt");
        assert_eq!(count.count, 1);
        assert_eq!(count.total_msats, 15_000);
    }

    #[test]
    fn multiple_receipts_to_same_target_sum_and_count() {
        let proj = ZapsAggregateProjection::new();
        proj.on_kernel_event(&receipt("Z1", "NOTE", 10_000, Some("alice")));
        proj.on_kernel_event(&receipt("Z2", "NOTE", 20_000, Some("bob")));
        proj.on_kernel_event(&receipt("Z3", "NOTE", 30_000, Some("carol")));

        let count = proj.snapshot().totals.remove("NOTE").expect("NOTE present");
        assert_eq!(count.count, 3);
        assert_eq!(count.total_msats, 60_000);
    }

    #[test]
    fn receipts_to_different_targets_are_indexed_separately() {
        let proj = ZapsAggregateProjection::new();
        proj.on_kernel_event(&receipt("Z1", "NOTE_A", 10_000, Some("alice")));
        proj.on_kernel_event(&receipt("Z2", "NOTE_B", 25_000, Some("bob")));
        proj.on_kernel_event(&receipt("Z3", "NOTE_A", 5_000, Some("carol")));

        let snap = proj.snapshot();
        let a = snap.totals.get("NOTE_A").expect("NOTE_A present");
        let b = snap.totals.get("NOTE_B").expect("NOTE_B present");
        assert_eq!(a.count, 2);
        assert_eq!(a.total_msats, 15_000);
        assert_eq!(b.count, 1);
        assert_eq!(b.total_msats, 25_000);
    }

    #[test]
    fn duplicate_receipt_id_does_not_double_count() {
        // A relay re-delivering the same receipt id must not inflate either
        // `count` or `total_msats` — dedupe on receipt id is mandatory.
        let proj = ZapsAggregateProjection::new();
        let r = receipt("Z1", "NOTE", 15_000, Some("alice"));
        proj.on_kernel_event(&r);
        proj.on_kernel_event(&r);
        proj.on_kernel_event(&r);

        let count = proj.snapshot().totals.remove("NOTE").expect("NOTE present");
        assert_eq!(count.count, 1, "re-delivered receipt must not duplicate");
        assert_eq!(count.total_msats, 15_000);
    }

    #[test]
    fn non_receipt_kinds_are_ignored() {
        let proj = ZapsAggregateProjection::new();
        // kind:1 (plain note) with an `e` tag pointing at NOTE — must not
        // accumulate, the projection is kind:9735-only.
        let note = KernelEvent {
            id: "N1".into(),
            author: "alice".into(),
            kind: 1,
            created_at: 1,
            tags: vec![vec!["e".into(), "NOTE".into()]],
            content: "hello".into(),
        };
        // kind:9734 (zap request) — also not a receipt.
        let request = KernelEvent {
            id: "ZR".into(),
            author: "alice".into(),
            kind: 9734,
            created_at: 1,
            tags: vec![
                vec!["p".into(), "recipient".into()],
                vec!["e".into(), "NOTE".into()],
            ],
            content: String::new(),
        };
        proj.on_kernel_event(&note);
        proj.on_kernel_event(&request);

        assert!(
            proj.snapshot().totals.is_empty(),
            "non-9735 events must not contribute"
        );
    }

    #[test]
    fn receipt_without_e_tag_is_ignored() {
        // A profile zap (no `e` tag, only `p`) is a valid kind:9735 but out
        // of scope for this projection — it indexes by target event id.
        let proj = ZapsAggregateProjection::new();
        let profile_zap = KernelEvent {
            id: "ZP".into(),
            author: "ln_node".into(),
            kind: 9735,
            created_at: 1,
            tags: vec![
                vec!["p".into(), "recipient".into()],
                vec!["bolt11".into(), "lnbc10n1pvj...".into()],
            ],
            content: String::new(),
        };
        proj.on_kernel_event(&profile_zap);
        assert!(proj.snapshot().totals.is_empty());
    }

    #[test]
    fn receipt_with_no_parseable_amount_counts_but_contributes_zero_msats() {
        // A receipt that has neither a parseable bolt11 nor an embedded
        // request `amount` carries `amount_msats == None` from the decoder.
        // The projection still counts the zap (it happened) but adds 0 msats
        // — matches the `ZapsView` precedent (`msats: record.amount_msats.unwrap_or(0)`).
        let proj = ZapsAggregateProjection::new();
        let no_amount = KernelEvent {
            id: "ZN".into(),
            author: "ln_node".into(),
            kind: 9735,
            created_at: 1,
            tags: vec![
                vec!["p".into(), "recipient".into()],
                vec!["e".into(), "NOTE".into()],
            ],
            content: String::new(),
        };
        proj.on_kernel_event(&no_amount);

        let count = proj.snapshot().totals.remove("NOTE").expect("NOTE present");
        assert_eq!(count.count, 1);
        assert_eq!(count.total_msats, 0);
    }

    #[test]
    fn snapshot_json_shape_is_a_named_totals_field() {
        // The wire shape the Chirp follow-up PR will consume: a top-level
        // object with a `totals` field mapping target id → {total_msats, count}.
        // Pin the shape here so an accidental rename of the field name or
        // ZapCount fields would fail this test loudly.
        let proj = ZapsAggregateProjection::new();
        proj.on_kernel_event(&receipt("Z1", "NOTE", 15_000, Some("alice")));

        let json = proj.snapshot_json();
        let totals = json
            .get("totals")
            .and_then(|t| t.as_object())
            .expect("snapshot json has a `totals` object");
        let note = totals
            .get("NOTE")
            .and_then(|n| n.as_object())
            .expect("totals contains NOTE");
        assert_eq!(
            note.get("total_msats").and_then(|v| v.as_u64()),
            Some(15_000)
        );
        assert_eq!(note.get("count").and_then(|v| v.as_u64()), Some(1));
    }

    #[test]
    fn round_trips_through_serde() {
        let proj = ZapsAggregateProjection::new();
        proj.on_kernel_event(&receipt("Z1", "NOTE", 15_000, Some("alice")));
        proj.on_kernel_event(&receipt("Z2", "NOTE", 25_000, Some("bob")));
        let snap = proj.snapshot();
        let encoded = serde_json::to_string(&snap).expect("snapshot serialises");
        let decoded: ZapsAggregateSnapshot =
            serde_json::from_str(&encoded).expect("snapshot deserialises");
        assert_eq!(snap, decoded);
    }

    #[test]
    fn drives_through_observer_trait_object() {
        // The projection must be usable as `Arc<dyn KernelEventObserver>` —
        // that is exactly how a host registers it with `register_event_observer`.
        let proj = Arc::new(ZapsAggregateProjection::new());
        let observer: Arc<dyn KernelEventObserver> = Arc::clone(&proj) as _;
        observer.on_kernel_event(&receipt("Z1", "NOTE", 10_000, Some("alice")));
        let count = proj.snapshot().totals.remove("NOTE").expect("NOTE present");
        assert_eq!(count.count, 1);
        assert_eq!(count.total_msats, 10_000);
    }
}
