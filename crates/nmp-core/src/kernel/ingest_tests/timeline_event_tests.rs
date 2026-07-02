//! `ingest_timeline_event` (kind:1) unit tests: subscribed-author admission
//! into the `events` cache and `timeline` ordering projection, the ADR-0070
//! persist-without-project oracle for non-followed authors, and duplicate-
//! delivery idempotence.

use super::ingest_support::signed_note;
use super::*;

// F-CR-00: `ingest_timeline_event_queues_missing_author_profile_request` and
// `ingest_timeline_event_skips_author_profile_when_cached` were deleted when
// the proactive kind:0 fetch at timeline.rs:172 was removed. The replacement
// invariants live in `proactive_profile_fetch_tests.rs`:
//   - `kind1_ingest_does_not_queue_profile_fetch` (no proactive fetch)
//   - `resolve_profile_after_ingest_queues_fetch` (resolve path works)

/// A signed kind:1 from an author present in `timeline_authors` passes the
/// timeline gate: it is persisted to the `events` read-cache AND appended to
/// the `timeline` ordering projection.
///
/// The sub_id (`follow-feed-default`) is a plain id with none of the
/// gate-bypass prefixes (`diag-firehose-`, `author-notes-`, `thread-*`), so
/// the author membership in `timeline_authors` is the only thing that opens
/// both the `should_store_event` gate and the `timeline.push_back` gate.
#[test]
fn ingest_timeline_event_from_subscribed_author_stores_event() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let keys = ::nostr::Keys::generate();
    let event = signed_note(&keys, "hello from a followed author", 1_700_000_000);
    let event_id = event.id.clone();

    // Subscribe the author: child-module test access to the kernel-private
    // `timeline_authors` projection (the `*_for_test` accessor is read-only).
    kernel.timeline_authors.insert(event.pubkey.clone());

    kernel.ingest_timeline_event(
        RelayRole::Content,
        "wss://relay.example/",
        "follow-feed-default",
        event,
    );

    assert!(
        kernel.events.contains_key(&event_id),
        "a signed kind:1 from a subscribed author must be cached in `events`",
    );
    assert!(
        kernel.timeline.iter().any(|id| id == &event_id),
        "a subscribed author's event must also be appended to the `timeline` \
         ordering projection",
    );
}

/// ADR-0070 oracle — a signed kind:1 from an author NOT in `timeline_authors`
/// (and not matched by any `should_store_event` read-time bypass) PERSISTS to
/// the authoritative store (admission = valid signature; persistence is no
/// longer relevance-gated — #1442) but does NOT enter the timeline VIEW: the
/// read-cache (`self.events`) and the ordering projection (`self.timeline`)
/// stay empty. Persistence ≠ projection.
#[test]
fn non_subscribed_author_event_persists_but_does_not_timeline_project() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // No active account — no implicit gate openings.

    let keys = ::nostr::Keys::generate();
    let event = signed_note(&keys, "note from a stranger", 1_700_000_100);
    let event_id = event.id.clone();

    // Author is deliberately NOT inserted into `timeline_authors`.
    kernel.ingest_timeline_event(
        RelayRole::Content,
        "wss://relay.example/",
        "follow-feed-default",
        event,
    );

    // ADR-0070 — the event IS now in the authoritative store (kind-agnostic,
    // valid-sig admission). This closes the #1442 relevance-shaped hole.
    let id_bytes = crate::kernel::hex_to_pubkey_bytes(&event_id).expect("event id is 64-char hex");
    assert!(
        kernel
            .store
            .get_by_id(&id_bytes)
            .expect("store get_by_id must not error")
            .is_some(),
        "ADR-0070: a validly-signed non-followed-author event must be PERSISTED",
    );

    // …but it is NOT projected into the timeline VIEW.
    assert!(
        kernel.events.is_empty(),
        "a non-followed author's event must NOT enter the timeline read-cache",
    );
    assert!(
        kernel.timeline.is_empty(),
        "a non-followed author's event must NOT enter the timeline ordering",
    );
}

/// A duplicate ingest of the same signed event (same id, same relay) is not
/// double-stored: the second delivery hits `InsertOutcome::Duplicate` and
/// returns before the `events.insert` / `timeline.push_back`, so both
/// projections still hold exactly one entry.
#[test]
fn ingest_timeline_event_duplicate_is_not_double_stored() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let keys = ::nostr::Keys::generate();
    let event = signed_note(&keys, "ingested twice", 1_700_000_200);
    let event_id = event.id.clone();
    kernel.timeline_authors.insert(event.pubkey.clone());

    // First delivery → Inserted.
    kernel.ingest_timeline_event(
        RelayRole::Content,
        "wss://relay.example/",
        "follow-feed-default",
        event.clone(),
    );
    // Second delivery, identical event from the same relay → Duplicate.
    kernel.ingest_timeline_event(
        RelayRole::Content,
        "wss://relay.example/",
        "follow-feed-default",
        event,
    );

    assert_eq!(
        kernel.events.len(),
        1,
        "a duplicate ingest must not add a second `events` cache entry",
    );
    assert_eq!(
        kernel.timeline.len(),
        1,
        "a duplicate ingest must not append a second `timeline` entry",
    );
    assert!(
        kernel.events.contains_key(&event_id),
        "the single cached event is the one that was ingested",
    );
}
