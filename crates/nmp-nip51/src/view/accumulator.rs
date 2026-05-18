//! Shared `ListAccumulator` used by both `ListView` and `ListDetailView`.
//!
//! List-aware accumulator: holds the latest list per `(author, kind, d_tag)`
//! (NIP-33 / replaceable — newest `created_at` wins) keyed on that triple;
//! produces `created_at`-desc-sorted `Vec<ListRecord>` snapshots. Decoding
//! happens once at insert via [`try_from_kernel_event`] (D8 hot-path — no
//! per-event allocation beyond the owned record).

use std::collections::BTreeMap;

use nmp_core::substrate::{EventId, KernelEvent};
use serde::{Deserialize, Serialize};

use crate::decode::{try_from_kernel_event, ListRecord};

/// `(author, kind, d_tag)` — the NIP-33 / replaceable identity. `kind` is part
/// of the key because this crate spans six kinds; a mute list and a relay list
/// by one author both have `d_tag == ""` and must not alias.
type ListKey = (String, u32, String);

/// In-memory state for the view modules. The inner map is private so external
/// mutation goes through the replaceability-enforcing API.
#[derive(Default)]
pub struct ListAccumulator {
    records: BTreeMap<ListKey, ListRecord>,
}

impl ListAccumulator {
    fn key_of(record: &ListRecord) -> ListKey {
        (
            record.author.clone(),
            record.list_kind.kind(),
            record.d_tag.clone(),
        )
    }

    /// Insert (or replace) the list decoded from `event`. Returns the delta —
    /// `None` if the event is not a valid NIP-51 list or is staler than the
    /// existing record for the same `(author, kind, d_tag)` triple.
    pub fn insert(&mut self, event: &KernelEvent) -> Option<ListViewDelta> {
        let record = try_from_kernel_event(event)?;
        let key = Self::key_of(&record);
        if let Some(existing) = self.records.get(&key) {
            if existing.created_at >= record.created_at {
                return None;
            }
        }
        self.records.insert(key, record);
        Some(ListViewDelta::Updated(event.id.clone()))
    }

    /// Remove the list whose `event_id` matches `id`.
    pub fn remove(&mut self, id: &EventId) -> Option<ListViewDelta> {
        let removed_key = self
            .records
            .iter()
            .find(|(_, r)| r.event_id == *id)
            .map(|(k, _)| k.clone())?;
        self.records.remove(&removed_key);
        Some(ListViewDelta::Removed(id.clone()))
    }

    /// Replace the list keyed on `old_id` with the new event's decoded form.
    pub fn replace(&mut self, old_id: &EventId, event: &KernelEvent) -> Option<ListViewDelta> {
        self.remove(old_id);
        self.insert(event)
    }

    /// All lists sorted by `created_at` desc.
    #[must_use]
    pub fn snapshot_sorted(&self) -> Vec<ListRecord> {
        let mut out: Vec<ListRecord> = self.records.values().cloned().collect();
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        out
    }
}

/// Delta emitted by the accumulator on every state-changing operation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ListViewDelta {
    /// The list with this event id was inserted or replaced.
    Updated(EventId),
    /// The list with this event id was removed.
    Removed(EventId),
}
