//! V-103: D1 Offline Bootstrap Regression Test
//!
//! Doctrine D1 mandates that the first rendered snapshot must precede any relay
//! I/O — the kernel must emit an initial update frame from offline-stored events
//! BEFORE dialing any relays.
//!
//! This test exercises the real offline read path:
//!
//! 1. Seed the in-memory store with a kind:1 event — NO relays connected.
//! 2. Open the timeline view (sets `follow_feed_kinds`) so the projection is live.
//! 3. Call `make_update_json_for_test` and assert the seeded event id AND content
//!    appear in `projections.timeline`.
//!
//! The test FAILS if the offline store-read/projection path breaks because the
//! assertion checks concrete seeded content, not just structural JSON presence.
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

/// D1 assertion: a kernel with locally-stored events emits those events in the
/// timeline projection BEFORE any relay I/O.
///
/// The test seeds a kind:1 note into the kernel's in-memory store with ZERO
/// relay connections, opens the timeline view, and asserts that the seeded
/// event id and content appear in `projections.timeline`.
///
/// Falsifiability: if the offline store-read path or the timeline projection
/// breaks, `projections.timeline` will be empty or missing the seeded entry,
/// and the `assert_eq!(items.len(), 1)` / id / content assertions will fail.
/// The tautological structural-presence check (`!projections.is_empty()`) has
/// been deliberately replaced with content-level assertions that cannot pass
/// on a kernel whose store-read path is severed.
#[test]
fn d1_offline_store_content_appears_in_snapshot_without_relays() {
    // Construct a kernel — zero relay connections, zero relay URLs configured.
    // `relay_connected()` is intentionally NOT called; this is the offline state.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Open the timeline view so the projection is emitted in the snapshot (D5
    // bounding rule: view-dependent keys are absent until a view is subscribed).
    // Mirrors what `ActorCommand::OpenContactListSubscription` does in production.
    kernel.follow_feed_kinds = std::collections::BTreeSet::from([1u32, 6u32]);

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

    // Emit the snapshot — same call the actor makes on every kernel tick.
    let snapshot_json = kernel.make_update_json_for_test(true);
    let snap: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("snapshot must be valid JSON");

    // ── D1 content assertion ──────────────────────────────────────────────────
    // The timeline projection must contain exactly the seeded note.  Any
    // regression in the offline store-read path (store.insert, events HashMap,
    // timeline VecDeque, or the projection loop in update/projections.rs) will
    // produce an empty or absent array and fail here.
    let items = snap["projections"]["timeline"]
        .as_array()
        .unwrap_or_else(|| {
            panic!(
                "D1: projections.timeline must be a JSON array with offline-stored content; \
                 got snapshot projections keys: {:?}",
                snap["projections"]
                    .as_object()
                    .map(|o| o.keys().collect::<Vec<_>>())
                    .unwrap_or_default()
            )
        });

    assert_eq!(
        items.len(),
        1,
        "D1: exactly one seeded note must appear in the timeline before any relay connects; \
         got {} items",
        items.len()
    );

    assert_eq!(
        items[0]["id"].as_str(),
        Some(SEED_NOTE_ID),
        "D1: the projected timeline item must carry the seeded event id"
    );

    assert_eq!(
        items[0]["content"].as_str(),
        Some(SEED_CONTENT),
        "D1: the projected timeline item must carry the seeded event content"
    );

    // The diagnostic metric must agree with the projection.
    assert_eq!(
        snap["metrics"]["note_events"].as_u64(),
        Some(1),
        "D1: metrics.note_events must count the seeded kind:1"
    );
}
