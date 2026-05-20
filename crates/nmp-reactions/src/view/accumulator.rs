//! Shared `ReactionAccumulator` used by `ReactionSummaryView` and
//! `RepostsView`.
//!
//! Keyed on `event_id` for **idempotency** (kinds 7/6/16 are regular events —
//! the id is the identity; re-inserting the same id is a no-op, never a
//! double-count). The per-`(reactor, target)` newest-wins collapse is computed
//! at snapshot time, not at insert, so the raw history stays available for
//! `RepostsView` while `ReactionSummaryView` sees the collapsed aggregate.
//!
//! Decode happens once per delivered event via `try_from_kernel_event` (D8
//! hot-path discipline — no per-event allocation beyond the owned record).

use std::collections::BTreeMap;

use nmp_core::substrate::{EventId, KernelEvent};
use serde::{Deserialize, Serialize};

use crate::decode::{try_from_kernel_event, ReactionKind, ReactionRecord};

/// In-memory state for the reaction/repost `ViewModule` impls. The inner map is
/// private so external mutation goes through `insert` / `remove`, which enforce
/// the event-id idempotency contract.
#[derive(Default)]
pub struct ReactionAccumulator {
    /// `event_id` → decoded record. The map key gives free idempotency.
    records: BTreeMap<String, ReactionRecord>,
}

impl ReactionAccumulator {
    /// Insert the record decoded from `event`. Returns the delta — `None` if
    /// the event is not a valid reaction/repost or the same `event_id` is
    /// already present (idempotent: identical id never double-counts).
    pub fn insert(&mut self, event: &KernelEvent) -> Option<ReactionViewDelta> {
        let record = try_from_kernel_event(event)?;
        if self.records.contains_key(&record.event_id) {
            // Same immutable event re-delivered (reconnect backfill, multi-relay
            // fan-in). Idempotent no-op — this is the regular-event analogue of
            // nip23's stale-redelivery guard.
            return None;
        }
        let id = record.event_id.clone();
        self.records.insert(id.clone(), record);
        Some(ReactionViewDelta::Updated(id))
    }

    /// Remove the record whose `event_id` matches `id`.
    pub fn remove(&mut self, id: &EventId) -> Option<ReactionViewDelta> {
        self.records
            .remove(id)
            .map(|_| ReactionViewDelta::Removed(id.clone()))
    }

    /// Replace the record keyed on `old_id` with the new event's decoded form.
    pub fn replace(&mut self, old_id: &EventId, event: &KernelEvent) -> Option<ReactionViewDelta> {
        self.remove(old_id);
        self.insert(event)
    }

    /// All records newest-first (by `created_at` desc, then `event_id` for
    /// determinism). Deterministic so SwiftUI diffing is stable (D8).
    pub fn snapshot_records(&self) -> Vec<ReactionRecord> {
        let mut out: Vec<ReactionRecord> = self.records.values().cloned().collect();
        out.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| a.event_id.cmp(&b.event_id))
        });
        out
    }

    /// Aggregate the reaction *content* counts with a per-reactor newest-wins
    /// collapse. Only kind:7 reactions are counted (reposts are surfaced by
    /// `RepostsView`, not the reaction summary). Output is `(content, count)`
    /// sorted by count desc then content asc (stable ordering for diffing) plus
    /// the total distinct-reactor count.
    pub fn reaction_summary(&self) -> (Vec<(String, u64)>, u64) {
        // newest-first ensures the first record seen per reactor is its newest.
        let ordered = self.snapshot_records();
        let mut newest_per_reactor: BTreeMap<String, &ReactionRecord> = BTreeMap::new();
        for r in &ordered {
            if matches!(r.kind, ReactionKind::Reaction { .. }) {
                newest_per_reactor.entry(r.author.clone()).or_insert(r);
            }
        }
        let mut counts: BTreeMap<String, u64> = BTreeMap::new();
        for r in newest_per_reactor.values() {
            if let ReactionKind::Reaction { content, .. } = &r.kind {
                *counts.entry(content.clone()).or_insert(0) += 1;
            }
        }
        let total: u64 = counts.values().sum();
        let mut entries: Vec<(String, u64)> = counts.into_iter().collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        (entries, total)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ReactionViewDelta {
    Updated(EventId),
    Removed(EventId),
}
