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
//! - **`register_typed_snapshot_projection`** — the *output* side (ADR-0037).
//!   A host registers a no-argument closure that runs on every snapshot tick
//!   and returns a typed FlatBuffers sidecar (`TypedProjectionData`) under a
//!   host-chosen key, or `None` when there is no changed row to emit.
//!
//! `ZapsAggregateProjection` is built to sit on *both*: it implements
//! `KernelEventObserver` for ingest, and exposes
//! [`ZapsAggregateProjection::snapshot`] — a cheap, non-blocking,
//! no-argument read — so the host can encode it into a typed sidecar and
//! register it as
//!
//! ```ignore
//! let projection = Arc::new(ZapsAggregateProjection::new());
//! let observer_id = app.register_event_observer(
//!     Arc::clone(&projection) as Arc<dyn KernelEventObserver>,
//! );
//! let snap = Arc::clone(&projection);
//! app.register_typed_snapshot_projection("nmp.nip57.zaps", move || {
//!     zaps_typed_projection(&snap)
//! });
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

use nmp_core::substrate::{BoundedMessageMap, EventId, KernelEvent, MAX_PROJECTION_MESSAGES};
use nmp_core::KernelEventObserver;
use serde::{Deserialize, Serialize};

use crate::pending::try_from_kernel_event_validated;

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
    #[must_use]
    pub fn snapshot(&self) -> ZapsAggregateSnapshot {
        let Ok(by_target) = self.by_target.lock() else {
            return ZapsAggregateSnapshot::empty();
        };
        let totals: HashMap<EventId, ZapCount> = by_target
            .iter()
            .map(|(target, receipts)| {
                let count = u32::try_from(receipts.len()).unwrap_or(u32::MAX);
                let total_msats = receipts.values().sum();
                (target.clone(), ZapCount { total_msats, count })
            })
            .collect();
        ZapsAggregateSnapshot { totals }
    }

    /// Snapshot as a `serde_json::Value` — the exact shape a host
    /// `register_snapshot_projection` closure must return.
    ///
    /// D6: a serialisation failure (not expected for this plain struct)
    /// collapses to `json!({"totals": {}})` rather than propagating.
    #[must_use]
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
        let Some(record) = try_from_kernel_event_validated(event) else {
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
mod tests;
