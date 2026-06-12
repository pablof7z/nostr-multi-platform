//! Free helper functions shared within the `kernel::ingest` module.
//!
//! Extracted from `mod.rs` to keep it under the file-size gate.

use super::super::NostrEvent;

/// Returns up to the first 16 chars of an event id, safe for any length.
pub(super) fn event_short_id(id: &str) -> &str {
    &id[..id.len().min(16)]
}

/// Project a wire-parsed [`NostrEvent`] into the store's [`crate::store::RawEvent`].
///
/// The signed-event tap, `verify_and_persist`, and `ingest_timeline_event`
/// each need an identical `RawEvent` to feed `VerifiedEvent::try_from_raw` —
/// this is the single construction site so the field list never drifts.
pub(super) fn raw_event_from_nostr(event: &NostrEvent) -> crate::store::RawEvent {
    crate::store::RawEvent {
        id: event.id.clone(),
        pubkey: event.pubkey.clone(),
        created_at: event.created_at,
        kind: event.kind,
        tags: event.tags.clone(),
        content: event.content.clone(),
        sig: event.sig.clone(),
    }
}

pub(super) fn raw_tap_should_fire(outcome: &crate::store::InsertOutcome) -> bool {
    use crate::store::InsertOutcome;
    matches!(
        outcome,
        InsertOutcome::Inserted { .. }
            | InsertOutcome::Duplicate { .. }
            | InsertOutcome::Replaced { .. }
            | InsertOutcome::Ephemeral { .. }
    )
}

pub(super) fn kernel_event_from_nostr(event: &NostrEvent) -> crate::substrate::KernelEvent {
    crate::substrate::KernelEvent {
        id: event.id.clone(),
        author: event.pubkey.clone(),
        kind: event.kind,
        created_at: event.created_at,
        tags: event.tags.clone(),
        content: event.content.clone(),
    }
}
