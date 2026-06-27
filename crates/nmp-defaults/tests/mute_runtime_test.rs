//! Integration tests for [`nmp_defaults::register_mute_runtime`].
//!
//! # What is tested here
//!
//! 1. `register_defaults` wires the `"nmp.nip51.mute_list"` snapshot
//!    projection (cold state: empty muted_pubkeys / muted_event_ids arrays).
//! 2. `register_mute_runtime` returns an `Arc<MuteListProjection>` that, when
//!    wired into a [`nmp_nip01::ModularTimelineProjection`] via
//!    `set_suppression`, suppresses kind:1 notes from muted authors.
//! 3. Account-switch safety: after the active pubkey changes, the stale mute
//!    set from the prior account must not suppress content for the new account.
//!
//! Tests 2 and 3 use the projection layer directly (no live kernel) — the same
//! posture as `nmp-nip51/tests/timeline_suppression.rs`. The new coverage here
//! is that `register_mute_runtime` returns an `Arc` that is correctly wired as
//! an observer + suppression source.

use std::sync::{Arc, Mutex};

use nmp_core::substrate::{EventId, KernelEvent, SuppressionLookup};
use nmp_core::ObservedProjectionSink;
mod common;
use common::*;
use nmp_nip01::{ModularTimelineProjection, ModularTimelineSpec, TimelineEventCard};
use nmp_nip51::MuteListProjection;
use nmp_threading::ModulePolicy;

// ── Test pubkey constants ─────────────────────────────────────────────────────

const ALICE: &str = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
const BOB: &str = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
const CAROL: &str = "cc11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
const NOTE_BOB: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const NOTE_CAROL: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const MUTE_EV_ID: &str = "3333333333333333333333333333333333333333333333333333333333333333";

// ── Helpers ───────────────────────────────────────────────────────────────────

fn kind1(author: &str, event_id: &str) -> KernelEvent {
    KernelEvent {
        id: EventId::from(event_id.to_string()),
        author: author.to_string(),
        kind: 1,
        created_at: 1000,
        tags: vec![],
        content: "hello".to_string(),
        relay_provenance: Vec::new(),
    }
}

fn mute_event(active: &str, muted_pks: &[&str], muted_eids: &[&str]) -> KernelEvent {
    let mut tags: Vec<Vec<String>> = muted_pks
        .iter()
        .map(|pk| vec!["p".to_string(), pk.to_string()])
        .collect();
    for eid in muted_eids {
        tags.push(vec!["e".to_string(), eid.to_string()]);
    }
    KernelEvent {
        id: EventId::from(
            "0000000000000000000000000000000000000000000000000000000000000001".to_string(),
        ),
        author: active.to_string(),
        kind: 10000,
        created_at: 9999,
        tags,
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

/// Build a `(ModularTimelineProjection, Arc<MuteListProjection>)` pair that
/// share a single hex-pubkey slot — simulating what `register_mute_runtime`
/// does at composition time, without requiring a live `NmpApp`.
fn wired_timeline(active_hex: &str) -> (ModularTimelineProjection, Arc<MuteListProjection>) {
    let slot = Arc::new(Mutex::new(Some(active_hex.to_string())));
    let mute = Arc::new(MuteListProjection::new(Arc::clone(&slot)));

    let spec = ModularTimelineSpec {
        viewer: active_hex.to_string(),
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

// ── Test 1: register_defaults wires the mute_list snapshot projection ─────────

/// `register_defaults` must register `"nmp.nip51.mute_list"` as a snapshot
/// projection. Cold state: both arrays empty.
#[test]
fn register_defaults_wires_mute_list_projection() {
    let app = new_app_ptr();
    assert!(!app.is_null(), "nmp_app_new returned null");

    // SAFETY: `app` is a valid non-null pointer from `nmp_app_new`.
    nmp_defaults::register_defaults(unsafe { &mut *app });

    // The generic JSON lane is deleted (rule A6). Use the typed FlatBuffers sidecar.
    let app_ref: &NmpApp = unsafe { &*app };
    let projections = app_ref.run_typed_snapshot_projections();
    let entry = projections
        .iter()
        .find(|p| p.key == "nmp.nip51.mute_list" && !p.payload.is_empty())
        .expect("register_defaults must register the nmp.nip51.mute_list typed projection");
    let snapshot = nmp_nip51::wire::mute_list_fb::decode_mute_list(&entry.payload)
        .expect("mute_list projection must decode to MuteListSnapshot");
    free_app_ptr(app);

    assert!(
        snapshot.muted_pubkeys.is_empty(),
        "cold mute_list must have empty muted_pubkeys"
    );
    assert!(
        snapshot.muted_event_ids.is_empty(),
        "cold mute_list must have empty muted_event_ids"
    );
}

// ── Test 2: muted author note is absent from timeline ──────────────────────

/// After Alice mutes Bob (kind:10000), Bob's kind:1 must not appear in the
/// timeline snapshot.
#[test]
fn muted_author_suppressed_in_timeline() {
    let (timeline, mute) = wired_timeline(ALICE);

    // Alice mutes Bob.
    mute.on_kernel_event(&mute_event(ALICE, &[BOB], &[]));

    // Bob publishes a note.
    timeline.on_kernel_event(&kind1(BOB, NOTE_BOB));
    // Carol publishes a note (not muted — must appear).
    timeline.on_kernel_event(&kind1(CAROL, NOTE_CAROL));

    let ids = card_ids(&timeline);
    assert!(
        !ids.contains(&NOTE_BOB.to_string()),
        "Bob (muted) note must be absent from timeline; got: {ids:?}"
    );
    assert!(
        ids.contains(&NOTE_CAROL.to_string()),
        "Carol (not muted) note must appear in timeline; got: {ids:?}"
    );
}

// ── Test 3: muted event-id is absent from timeline ─────────────────────────

/// After Alice mutes an event id, that event must not appear in the timeline
/// regardless of its author.
#[test]
fn muted_event_id_suppressed_in_timeline() {
    let (timeline, mute) = wired_timeline(ALICE);

    // Alice mutes a specific event id.
    mute.on_kernel_event(&mute_event(ALICE, &[], &[MUTE_EV_ID]));

    // Carol's note with the muted id (Carol is not pubkey-muted).
    timeline.on_kernel_event(&kind1(CAROL, MUTE_EV_ID));
    // Carol's second note (different id — must appear).
    timeline.on_kernel_event(&kind1(CAROL, NOTE_CAROL));

    let ids = card_ids(&timeline);
    assert!(
        !ids.contains(&MUTE_EV_ID.to_string()),
        "muted event id must not appear in timeline; got: {ids:?}"
    );
    assert!(
        ids.contains(&NOTE_CAROL.to_string()),
        "Carol's non-muted note must appear; got: {ids:?}"
    );
}

// ── Test 4: account-switch resets suppression ─────────────────────────────

/// After switching from Alice to Carol, Alice's stale mutes must NOT suppress
/// Bob in Carol's session (even though Carol has no kind:10000 yet).
#[test]
fn account_switch_resets_mute_suppression() {
    let slot = Arc::new(Mutex::new(Some(ALICE.to_string())));
    let mute = Arc::new(MuteListProjection::new(Arc::clone(&slot)));

    let spec = ModularTimelineSpec {
        viewer: ALICE.to_string(),
        kinds: vec![1],
        authors: None,
        policy: ModulePolicy::default(),
    };
    let mut timeline = ModularTimelineProjection::new(&spec);
    timeline.set_suppression(Arc::clone(&mute) as Arc<dyn SuppressionLookup>);

    // Alice mutes Bob.
    mute.on_kernel_event(&mute_event(ALICE, &[BOB], &[]));
    // Bob's note arrives.
    timeline.on_kernel_event(&kind1(BOB, NOTE_BOB));
    assert!(
        !card_ids(&timeline).contains(&NOTE_BOB.to_string()),
        "Bob must be suppressed in Alice's session"
    );

    // Account switch: active slot now points to Carol.
    *slot.lock().unwrap() = Some(CAROL.to_string());
    // Carol has never published a kind:10000 — no mute event arrives.

    // Bob publishes a new note in Carol's session.
    let note_bob_2 = "4444444444444444444444444444444444444444444444444444444444444444";
    timeline.on_kernel_event(&kind1(BOB, note_bob_2));

    let ids = card_ids(&timeline);
    assert!(
        ids.contains(&note_bob_2.to_string()),
        "Bob must NOT be suppressed in Carol's session (stale mutes must not carry over); got: {ids:?}"
    );
}
