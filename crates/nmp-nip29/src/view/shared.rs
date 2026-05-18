//! Shared types used across the 7 ViewModule impls.
//!
//! Each view's `State` is a thin accumulator of ingested `KernelEvent`s; the
//! `Payload` is the snapshot ordered by `created_at` ascending; the `Delta`
//! is the per-event diff.
//!
//! Hydrated cross-protocol composition (e.g. joining `nip01::Profile`) lives
//! at the app layer per the M11.5 protocol-crate-isolation rule
//! (`nip29-crate.md` §6).

use nmp_core::substrate::{EventId, KernelEvent};
use serde::{Deserialize, Serialize};

/// Generic per-view accumulator: append-only list of events keyed by id.
#[derive(Default)]
pub struct EventAccumulator {
    pub events: Vec<KernelEvent>,
}

impl EventAccumulator {
    pub fn insert(&mut self, event: &KernelEvent) -> Option<EventAccumulatorDelta> {
        if self.events.iter().any(|e| e.id == event.id) {
            return None;
        }
        self.events.push(event.clone());
        // Keep sorted ascending by created_at for stable snapshots.
        self.events.sort_by_key(|e| e.created_at);
        Some(EventAccumulatorDelta::Inserted(event.id.clone()))
    }

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
