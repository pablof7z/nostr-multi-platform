//! Shared accumulator for the Marmot reactive views.
//!
//! Mirrors `nmp-nip29::view::shared`. Each view's `State` is a thin
//! append-only list of ingested `KernelEvent`s; `Payload` is the ordered
//! snapshot; `Delta` is the per-event diff.
//!
//! NOTE: Marmot kind:445 events are MLS-encrypted on the wire. The kernel's
//! raw ingest path sees only ciphertext envelopes. The decrypted projection
//! is materialised by [`crate::service`] after `process_message`, not inside
//! these view reducers. The view modules therefore ship correct trait
//! signatures + correct relay-pinned dependency declarations; the rich
//! decrypted projection is driven by the service + actor layer.

use nmp_core::substrate::{EventId, KernelEvent};
use serde::{Deserialize, Serialize};

/// Generic per-view accumulator: append-only list of events keyed by id,
/// kept sorted ascending by `created_at` for stable snapshots.
#[derive(Default)]
pub struct EventAccumulator {
    pub events: Vec<KernelEvent>,
}

impl EventAccumulator {
    #[must_use]
    pub fn insert(&mut self, event: &KernelEvent) -> Option<EventAccumulatorDelta> {
        if self.events.iter().any(|e| e.id == event.id) {
            return None;
        }
        self.events.push(event.clone());
        self.events.sort_by_key(|e| e.created_at);
        Some(EventAccumulatorDelta::Inserted(event.id.clone()))
    }

    #[must_use]
    pub fn remove(&mut self, id: &EventId) -> Option<EventAccumulatorDelta> {
        let before = self.events.len();
        self.events.retain(|e| e.id != *id);
        if self.events.len() == before {
            None
        } else {
            Some(EventAccumulatorDelta::Removed(id.clone()))
        }
    }

    pub fn replace(
        &mut self,
        old_id: &EventId,
        new_event: &KernelEvent,
    ) -> Option<EventAccumulatorDelta> {
        let pos = self.events.iter().position(|e| e.id == *old_id)?;
        self.events[pos] = new_event.clone();
        self.events.sort_by_key(|e| e.created_at);
        Some(EventAccumulatorDelta::Replaced {
            old_id: old_id.clone(),
            new_id: new_event.id.clone(),
        })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum EventAccumulatorDelta {
    Inserted(EventId),
    Removed(EventId),
    Replaced { old_id: EventId, new_id: EventId },
}
