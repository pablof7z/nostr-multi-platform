//! V-103: D1 Offline Bootstrap Regression Test
//!
//! Doctrine D1 mandates that the first rendered snapshot must precede any relay
//! I/O — the kernel must emit an initial update frame from offline-stored events
//! BEFORE dialing any relays.
//!
//! This test exercises the real offline read path:
//!
//! 1. Seed the in-memory store with a kind:1 event — NO relays connected.
//! 2. Call `ingest_pre_verified_event` + `sort_timeline_deferred` to populate
//!    the in-memory `events` and `timeline` caches exactly as the actor does.
//! 3. Assert the seeded event id appears in `kernel.events` and `kernel.timeline`.
//!
//! V-112 (ADR-0042): the previous observation window via
//! `projections.author_view.items` was deleted together with `AuthorViewState`.
//! The D1 property is unchanged — `self.timeline`, `self.events`, and
//! `ingest_pre_verified_event` all remain — so the test now observes them
//! directly (same fields `ingest_tests.rs` uses) rather than going through the
//! deleted projection.
//!
//! See `docs/product-spec/offline-first.md` §7 and
//! `docs/wiki/d1-snapshot-before-relay-io.md`.

use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::store::{RawEvent, VerifiedEvent};

// 64-hex constants for the seeded event and its author.
const SEED_NOTE_ID: &str =
    "d100000000000000000000000000000000000000000000000000000000000001";
const SEED_AUTHOR: &str =
    "d1aa0000000000000000000000000000000000000000000000000000000000aa";
const SEED_CONTENT: &str =
    "offline-first proof: this note was stored before any relay connected";

/// D1 assertion: a kernel with locally-stored events reflects them in the
/// `events` cache and `timeline` ordering BEFORE any relay I/O.
///
/// The test seeds a kind:1 note into the kernel's in-memory store with ZERO
/// relay connections and asserts that the seeded event id appears in both
/// `kernel.events` (the flat id→StoredEvent lookup map) and `kernel.timeline`
/// (the ordered id deque used for home-feed and FlatFeed projections).
///
/// Falsifiability: if the offline store-read path or the ingest_pre_verified
/// dispatch breaks, `kernel.events` will be empty or missing the seeded entry,
/// and the `kernel.events.contains_key(SEED_NOTE_ID)` assertion will fail. The
/// tautological structural-presence check has been deliberately replaced with
/// content-level assertions that cannot pass on a kernel whose ingest path is
/// severed.
#[test]
fn d1_offline_store_content_appears_in_snapshot_without_relays() {
    // Construct a kernel — zero relay connections, zero relay URLs configured.
    // `relay_connected()` is intentionally NOT called; this is the offline state.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Seed a kind:1 note directly into the kernel's store — bypasses signature
    // verification via `from_raw_unchecked` (test-support only).  The
    // `diag-firehose-` sub_id prefix is required: `ingest_pre_verified_event`
    // only appends to `self.timeline` for that prefix, mirroring the production
    // path where the actor drives timeline population.
    let raw = RawEvent {
        id: SEED_NOTE_ID.to_string(),
        pubkey: SEED_AUTHOR.to_string(),
        created_at: 1_700_000_000,
        kind: 1,
        tags: vec![],
        content: SEED_CONTENT.to_string(),
        sig: "a".repeat(128),
    };
    kernel.ingest_pre_verified_event(
        RelayRole::Content,
        "diag-firehose-stress",
        VerifiedEvent::from_raw_unchecked(raw),
    );
    kernel.sort_timeline_deferred();

    // ── D1 content assertion ──────────────────────────────────────────────────
    // The `events` read-cache must carry the seeded note verbatim, proving the
    // offline store-read path (store.insert → events.insert) is intact BEFORE
    // any relay dial.
    assert!(
        kernel.events.contains_key(SEED_NOTE_ID),
        "D1: kernel.events must contain the seeded note before any relay connects; \
         events keys: {:?}",
        kernel.events.keys().collect::<Vec<_>>()
    );

    let stored = &kernel.events[SEED_NOTE_ID];
    assert_eq!(
        stored.id, SEED_NOTE_ID,
        "D1: stored event id must match the seeded id"
    );
    assert_eq!(
        stored.content, SEED_CONTENT,
        "D1: stored event content must match the seeded content"
    );

    // The `timeline` deque (the ordering projection) must also carry the id,
    // confirming `sort_timeline_deferred` ran without relay I/O.
    assert!(
        kernel.timeline.iter().any(|id| id == SEED_NOTE_ID),
        "D1: kernel.timeline must contain the seeded note id after sort_timeline_deferred; \
         timeline: {:?}",
        kernel.timeline.iter().collect::<Vec<_>>()
    );

    // The diagnostic metric must agree — read it through the snapshot JSON to
    // avoid accessing the private `metric_note_events` field directly.
    let snapshot_json = kernel.make_update_json_for_test(true);
    let snap: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("snapshot must be valid JSON");
    assert_eq!(
        snap["metrics"]["note_events"].as_u64(),
        Some(1),
        "D1: metrics.note_events must count the seeded kind:1"
    );
}
