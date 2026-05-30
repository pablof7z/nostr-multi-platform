//! Integration test: seed a kind:10000 muting pubkey X, seed a kind:1 note
//! from X, assert the note is suppressed from the timeline projection.
//!
//! This is the primary acceptance criterion for V-42 — NIP-51 kind:10000
//! mute-list support with timeline suppression.
//!
//! The test wires `nmp-nip51::MuteListProjection` as both a
//! `KernelEventObserver` and a `SuppressionLookup` into an
//! `nmp-nip01::ModularTimelineProjection`. It exercises:
//!
//! 1. The ingest path: a kind:10000 event from the active account arrives
//!    via `on_kernel_event` — the muted pubkeys are recorded.
//! 2. The timeline suppression path: a kind:1 note from a muted author
//!    arrives — it is NOT inserted into the timeline cards.
//! 3. The snapshot path: the timeline snapshot contains no cards for the
//!    muted author.
//! 4. The read-time path: even events that arrived BEFORE the mute was
//!    applied are absent from the next snapshot.
//! 5. Event-id suppression: a kind:1 note whose event-id is muted is
//!    absent from the snapshot regardless of its author.

use std::sync::{Arc, Mutex};

use nmp_core::substrate::{EventId, KernelEvent, SuppressionLookup};
use nmp_core::KernelEventObserver;
use nmp_nip01::{ModularTimelineProjection, ModularTimelineSpec, TimelineEventCard};
use nmp_threading::ModulePolicy;
use nmp_nip51::MuteListProjection;

const ALICE: &str = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
const BOB: &str = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
const CAROL: &str = "cc11223344556677889900aabbccddeeff00112233445566778899aabbccddee";

const NOTE_BOB: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const NOTE_CAROL: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const NOTE_MUTED_ID: &str = "3333333333333333333333333333333333333333333333333333333333333333";

fn kind1_event(author: &str, event_id: &str, created_at: u64) -> KernelEvent {
    KernelEvent {
        id: EventId::from(event_id.to_string()),
        author: author.to_string(),
        kind: 1,
        created_at,
        tags: vec![],
        content: "hello".to_string(),
    }
}

fn mute_event(active: &str, muted_pubkeys: &[&str], muted_event_ids: &[&str]) -> KernelEvent {
    let mut tags: Vec<Vec<String>> = muted_pubkeys
        .iter()
        .map(|pk| vec!["p".to_string(), pk.to_string()])
        .collect();
    for eid in muted_event_ids {
        tags.push(vec!["e".to_string(), eid.to_string()]);
    }
    KernelEvent {
        id: EventId::from("0000000000000000000000000000000000000000000000000000000000000001".to_string()),
        author: active.to_string(),
        kind: 10000,
        created_at: 9999,
        tags,
        content: String::new(),
    }
}

fn timeline_with_mute(active: &str) -> (ModularTimelineProjection, Arc<MuteListProjection>) {
    let slot = Arc::new(Mutex::new(Some(active.to_string())));
    let mute = Arc::new(MuteListProjection::new(Arc::clone(&slot)));

    let spec = ModularTimelineSpec {
        viewer: ALICE.to_string(),
        kinds: vec![1],
        authors: None,
        policy: ModulePolicy::default(),
    };
    let mut timeline = ModularTimelineProjection::new(&spec);
    timeline.set_suppression(Arc::clone(&mute) as Arc<dyn SuppressionLookup>);
    (timeline, mute)
}

fn card_ids(proj: &ModularTimelineProjection) -> Vec<String> {
    proj.snapshot()
        .cards
        .into_iter()
        .map(|c: TimelineEventCard| c.id)
        .collect()
}

// ---------------------------------------------------------------------------
// Test 1: note from muted author is not present in timeline
// ---------------------------------------------------------------------------

#[test]
fn muted_author_note_absent_from_timeline() {
    let (timeline, mute) = timeline_with_mute(ALICE);

    // Alice mutes Bob.
    mute.on_kernel_event(&mute_event(ALICE, &[BOB], &[]));
    assert!(mute.is_suppressed_author(BOB));

    // Bob's note arrives at the timeline observer.
    timeline.on_kernel_event(&kind1_event(BOB, NOTE_BOB, 1000));

    // Carol's note arrives (not muted — must appear).
    timeline.on_kernel_event(&kind1_event(CAROL, NOTE_CAROL, 999));

    let ids = card_ids(&timeline);
    assert!(
        !ids.contains(&NOTE_BOB.to_string()),
        "Bob (muted) note must not appear in timeline; got: {ids:?}"
    );
    assert!(
        ids.contains(&NOTE_CAROL.to_string()),
        "Carol (not muted) note must appear in timeline; got: {ids:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 2: note already in timeline is suppressed after mute is applied
// (read-time suppression)
// ---------------------------------------------------------------------------

#[test]
fn retroactive_mute_suppresses_existing_timeline_card() {
    let (timeline, mute) = timeline_with_mute(ALICE);

    // Bob's note arrives BEFORE Alice mutes Bob.
    timeline.on_kernel_event(&kind1_event(BOB, NOTE_BOB, 1000));
    // Bob appears in the snapshot.
    assert!(
        card_ids(&timeline).contains(&NOTE_BOB.to_string()),
        "Bob's note must appear before the mute"
    );

    // Alice now mutes Bob.
    mute.on_kernel_event(&mute_event(ALICE, &[BOB], &[]));

    // On next snapshot Bob's card must be absent.
    let ids = card_ids(&timeline);
    assert!(
        !ids.contains(&NOTE_BOB.to_string()),
        "Bob (retroactively muted) note must disappear from timeline; got: {ids:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 3: muted event id suppresses a note regardless of its author
// ---------------------------------------------------------------------------

#[test]
fn muted_event_id_absent_from_timeline() {
    let (timeline, mute) = timeline_with_mute(ALICE);

    // Alice mutes a specific event id.
    mute.on_kernel_event(&mute_event(ALICE, &[], &[NOTE_MUTED_ID]));

    // Carol publishes that event id (Carol is not pubkey-muted).
    timeline.on_kernel_event(&kind1_event(CAROL, NOTE_MUTED_ID, 1000));
    // Carol publishes a second non-muted note.
    timeline.on_kernel_event(&kind1_event(CAROL, NOTE_CAROL, 999));

    let ids = card_ids(&timeline);
    assert!(
        !ids.contains(&NOTE_MUTED_ID.to_string()),
        "muted event id must not appear in timeline; got: {ids:?}"
    );
    assert!(
        ids.contains(&NOTE_CAROL.to_string()),
        "Carol's other note (not muted) must appear; got: {ids:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 4: empty mute list suppresses nothing
// ---------------------------------------------------------------------------

#[test]
fn empty_mute_list_suppresses_nothing() {
    let (timeline, mute) = timeline_with_mute(ALICE);

    // Alice publishes an empty kind:10000 (no mutes).
    mute.on_kernel_event(&mute_event(ALICE, &[], &[]));

    timeline.on_kernel_event(&kind1_event(BOB, NOTE_BOB, 1000));
    timeline.on_kernel_event(&kind1_event(CAROL, NOTE_CAROL, 999));

    let ids = card_ids(&timeline);
    assert!(ids.contains(&NOTE_BOB.to_string()), "Bob must appear with empty mute list");
    assert!(ids.contains(&NOTE_CAROL.to_string()), "Carol must appear with empty mute list");
}

// ---------------------------------------------------------------------------
// Test 5: unmuting restores the author to the timeline
// ---------------------------------------------------------------------------

#[test]
fn unmuting_restores_author_to_timeline_on_new_notes() {
    let (timeline, mute) = timeline_with_mute(ALICE);

    // Alice mutes Bob.
    mute.on_kernel_event(&mute_event(ALICE, &[BOB], &[]));

    // Bob's note (while muted — not in timeline).
    timeline.on_kernel_event(&kind1_event(BOB, NOTE_BOB, 1000));
    assert!(!card_ids(&timeline).contains(&NOTE_BOB.to_string()));

    // Alice unmutes Bob (publishes a replacement kind:10000 without Bob).
    mute.on_kernel_event(&mute_event(ALICE, &[], &[]));

    // Bob publishes a new note after unmute.
    let note_bob_2 = "4444444444444444444444444444444444444444444444444444444444444444";
    timeline.on_kernel_event(&kind1_event(BOB, note_bob_2, 1001));

    let ids = card_ids(&timeline);
    assert!(
        ids.contains(&note_bob_2.to_string()),
        "Bob must appear after unmute; got: {ids:?}"
    );
}
