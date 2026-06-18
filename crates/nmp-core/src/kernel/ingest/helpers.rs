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

/// Project a store [`crate::store::VerifiedEvent`] into the FFI-stable
/// [`crate::substrate::KernelEvent`]. The sibling of
/// [`kernel_event_from_nostr`] for callers that already hold a `VerifiedEvent`
/// (the unified post-store projection helper
/// [`crate::Kernel::project_accepted_event`], fed by BOTH the live chokepoint
/// and the cache-serve replay path) rather than a wire-parsed `NostrEvent`.
/// Produces a byte-identical observer payload to the `NostrEvent` builder so
/// the live-ingest and cache-serve fan-outs cannot diverge.
pub(in crate::kernel) fn kernel_event_from_verified(
    verified: &crate::store::VerifiedEvent,
) -> crate::substrate::KernelEvent {
    let raw = verified.raw();
    crate::substrate::KernelEvent {
        id: raw.id.clone(),
        author: raw.pubkey.clone(),
        kind: raw.kind,
        created_at: raw.created_at,
        tags: raw.tags.clone(),
        content: raw.content.clone(),
        relay_provenance: Vec::new(),
    }
}
