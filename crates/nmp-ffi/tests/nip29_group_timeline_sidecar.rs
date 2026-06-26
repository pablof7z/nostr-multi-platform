//! NIP-29 group-chat typed-projection sidecar proof.
//!
//! Proves `NmpApp::open_group_timeline` (#2088) emits a typed FlatBuffers sidecar
//! (ADR-0037, `NGTL`) under `"nmp.nip29.group_timeline"`. Drives the full FFI
//! snapshot path, decodes the frame with `decode_snapshot_typed_projections`,
//! and asserts the typed payload bytes land in the `typed_projections` sidecar,
//! round-tripping back through the generated bindings. (Live-ingest path:
//! events injected AFTER open. The catch-up / hydration path — events injected
//! BEFORE open — is covered by `nip29_hydration.rs`.)

mod common;

use common::{boot, inject, raw_event, teardown, wait_for_typed, HOST, SERIAL};

use nmp_store::VerifiedEvent;
use nmp_nip29::group_id::GroupId;
use nmp_nip29::{decode_group_timeline_snapshot, GROUP_TIMELINE_FILE_IDENTIFIER, GROUP_TIMELINE_SCHEMA_ID};

/// A kind:9 event h-tagged for the wired group surfaces in the
/// `"nmp.nip29.group_timeline"` typed sidecar with the `NGTL` identifier, and the
/// payload decodes back into the typed `GroupTimelineSnapshot`.
#[test]
fn group_timeline_typed_sidecar_round_trips() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let app = boot();

    // SAFETY: `app` is a valid pointer from `nmp_app_new`, live for this block.
    unsafe { (*app).open_group_timeline(GroupId::new(HOST, "test-room")) };

    let target = VerifiedEvent::from_raw_unchecked(raw_event(
        &"a".repeat(64),
        &"b".repeat(64),
        9,
        1_700_000_000,
        vec![vec!["h".into(), "test-room".into()]],
        "typed hello",
    ));
    inject(app, vec![target]);

    let entry = wait_for_typed("nmp.nip29.group_timeline", |t| {
        decode_group_timeline_snapshot(&t.payload)
            .map(|s| s.events.iter().any(|m| m.content == "typed hello"))
            .unwrap_or(false)
    })
    .expect("group_timeline typed sidecar must carry the injected message within 3 s");

    // Descriptor identity.
    assert_eq!(entry.schema_id, GROUP_TIMELINE_SCHEMA_ID);
    assert_eq!(
        entry.file_identifier,
        String::from_utf8_lossy(GROUP_TIMELINE_FILE_IDENTIFIER)
    );
    assert!(!entry.payload.is_empty(), "typed payload must carry bytes");

    // Full round-trip through the generated bindings.
    let snapshot = decode_group_timeline_snapshot(&entry.payload)
        .expect("NGTL payload must decode back into GroupTimelineSnapshot");
    let msg = snapshot
        .events
        .iter()
        .find(|m| m.content == "typed hello")
        .expect("decoded snapshot must contain the message");
    assert_eq!(msg.id, "a".repeat(64));
    assert_eq!(msg.pubkey, "b".repeat(64));
    assert_eq!(msg.created_at, 1_700_000_000);
    assert_eq!(msg.kind, 9);

    teardown(app);
}

/// The empty case: a wired-but-unfed group still emits a decodable `NGTL`
/// buffer with zero messages (the typed path emits every tick, in lockstep with
/// the generic path).
#[test]
fn group_timeline_typed_sidecar_empty_decodes() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let app = boot();

    unsafe { (*app).open_group_timeline(GroupId::new(HOST, "empty-room")) };

    let entry = wait_for_typed("nmp.nip29.group_timeline", |t| {
        decode_group_timeline_snapshot(&t.payload).is_ok()
    })
    .expect("group_timeline typed sidecar must appear even with no messages");

    let snapshot =
        decode_group_timeline_snapshot(&entry.payload).expect("empty NGTL payload must still decode");
    assert!(
        snapshot.events.is_empty(),
        "no events injected → empty messages, got {:?}",
        snapshot.events
    );

    teardown(app);
}
