//! `SuppressionLookup` — substrate-generic seam for the active account's
//! "suppress this author / this event" set.
//!
//! The timeline projection needs to ask "should this author or event be hidden
//! from the feed?" when building snapshots. The concrete suppression set
//! (NIP-51 kind:10000 mute list in the canonical case) lives in the
//! `nmp-nip51` crate so the kernel and projections never name the wire shape
//! of a kind:10000 event (D0 — `nmp-core` does not embed protocol nouns).
//!
//! The timeline projection holds an `Arc<dyn SuppressionLookup>` populated at
//! composition time; a projection built without any backend uses the
//! [`EmptySuppressionLookup`] default, which passes everything through.
//!
//! ## Why a trait, not a hardwired `HashSet`
//!
//! Mirrors the pattern [`BlockedRelayLookup`] uses for kind:10006:
//!
//! - The **writer** is `nmp-nip51`'s `MuteListProjection`
//!   ([`crate::KernelEventObserver`]) — registered at composition time.
//! - The **reader** is `nmp-nip01`'s `ModularTimelineProjection` — it
//!   consults this trait through the substrate-generic shape when building
//!   every snapshot. The timeline projection does NOT know the wire shape of
//!   a kind:10000 event.
//!
//! Both ends agree on a shared `Arc<MuteListProjection>` (the concrete type in
//! `nmp-nip51`) at composition time; the timeline projection sees it only as
//! `Arc<dyn SuppressionLookup>`.
//!
//! ## Fail-open contract
//!
//! `is_suppressed_author` / `is_suppressed_event` both default to `false`
//! (pass-through). A lookup that cannot consult its backing store (poisoned
//! mutex, no active account) must return `false`, not `true`. Suppressing
//! everything on error would hide the timeline; suppressing nothing is the
//! correct safe fallback.

use std::sync::Arc;

/// Lookup contract: given an event author hex pubkey or event-id hex, decide
/// whether the timeline projection should suppress this entry.
///
/// Implementations MUST:
///
/// - Return `false` ("do not suppress") on any error — poisoned mutex, missing
///   active account, or no mute list yet received. Fail-open is the correct
///   default for a safety feature (D6).
/// - Be cheap to call repeatedly — the snapshot path calls this once per card
///   in the visible window on every snapshot request, which happens on every
///   kernel tick.
/// - Use interior mutability for any backing store. The trait method takes
///   `&self`; the writer side (the kind:10000 ingest observer) drives
///   mutation through a different method on the concrete type.
pub trait SuppressionLookup: Send + Sync {
    /// Returns `true` if `author_pubkey` (lowercase hex) is on the active
    /// account's suppression set.
    fn is_suppressed_author(&self, author_pubkey: &str) -> bool;

    /// Returns `true` if `event_id` (lowercase hex) is on the active
    /// account's suppression set.
    fn is_suppressed_event(&self, event_id: &str) -> bool;
}

/// Default backing — always returns `false` for both queries. No suppression
/// is applied. Preserves the pre-NIP-51 behaviour byte-for-byte (the timeline
/// showed everything; this is the correct cold-start default).
#[derive(Default)]
pub struct EmptySuppressionLookup;

impl SuppressionLookup for EmptySuppressionLookup {
    fn is_suppressed_author(&self, _author_pubkey: &str) -> bool {
        false
    }

    fn is_suppressed_event(&self, _event_id: &str) -> bool {
        false
    }
}

/// Convenience: a fresh `Arc<dyn SuppressionLookup>` backed by
/// [`EmptySuppressionLookup`] — the default when no mute-list backend has
/// been wired in yet.
#[must_use]
pub fn empty_suppression_lookup() -> Arc<dyn SuppressionLookup> {
    Arc::new(EmptySuppressionLookup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::RwLock;

    /// Stand-in suppression set for unit-testing the trait contract.
    #[derive(Default)]
    struct TestLookup {
        suppressed_authors: RwLock<HashSet<String>>,
        suppressed_events: RwLock<HashSet<String>>,
    }

    impl TestLookup {
        fn suppress_author(&self, pubkey: &str) {
            self.suppressed_authors
                .write()
                .unwrap()
                .insert(pubkey.to_string());
        }
        fn suppress_event(&self, event_id: &str) {
            self.suppressed_events
                .write()
                .unwrap()
                .insert(event_id.to_string());
        }
    }

    impl SuppressionLookup for TestLookup {
        fn is_suppressed_author(&self, author_pubkey: &str) -> bool {
            self.suppressed_authors
                .read()
                .map(|g| g.contains(author_pubkey))
                .unwrap_or(false)
        }

        fn is_suppressed_event(&self, event_id: &str) -> bool {
            self.suppressed_events
                .read()
                .map(|g| g.contains(event_id))
                .unwrap_or(false)
        }
    }

    #[test]
    fn empty_lookup_never_suppresses() {
        let lookup: Arc<dyn SuppressionLookup> = empty_suppression_lookup();
        assert!(!lookup.is_suppressed_author("aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899"));
        assert!(!lookup.is_suppressed_event("0000000000000000000000000000000000000000000000000000000000000001"));
    }

    #[test]
    fn populated_lookup_suppresses_known_author() {
        let lookup = Arc::new(TestLookup::default());
        let pk = "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899";
        lookup.suppress_author(pk);
        assert!(lookup.is_suppressed_author(pk));
        assert!(!lookup.is_suppressed_author("other"));
    }

    #[test]
    fn populated_lookup_suppresses_known_event() {
        let lookup = Arc::new(TestLookup::default());
        let eid = "1111111111111111111111111111111111111111111111111111111111111111";
        lookup.suppress_event(eid);
        assert!(lookup.is_suppressed_event(eid));
        assert!(!lookup.is_suppressed_event("other"));
    }

    #[test]
    fn unknown_entries_are_not_suppressed() {
        let lookup = Arc::new(TestLookup::default());
        lookup.suppress_author("alice_pk");
        assert!(!lookup.is_suppressed_author("bob_pk"));
    }

    #[test]
    fn dyn_trait_object_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>(_: T) {}
        let lookup: Arc<dyn SuppressionLookup> = empty_suppression_lookup();
        assert_send_sync(lookup);
    }
}
