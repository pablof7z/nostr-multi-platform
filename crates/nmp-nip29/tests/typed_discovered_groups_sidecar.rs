//! NIP-29 discovered-groups typed-projection sidecar proof (Wave A producer-typing).
//!
//! Proves `nmp_nip29::register::open_group_discovery` emits a typed
//! FlatBuffers sidecar (ADR-0037, `NDGS`) ALONGSIDE the existing generic
//! `serde_json::Value` projection under `"nmp.nip29.discovered_groups"`. Drives
//! the full FFI snapshot path, decodes the frame with `decode_snapshot_typed_projections`,
//! and asserts the typed payload bytes land in the `typed_projections` sidecar,
//! round-tripping back through the generated bindings.

mod common;

use common::{boot, inject, raw_event, teardown, wait_for_typed, HOST, SERIAL};

use nmp_store::VerifiedEvent;
use nmp_nip29::register::open_group_discovery;
use nmp_nip29::{
    decode_discovered_groups_snapshot, DISCOVERED_GROUPS_FILE_IDENTIFIER,
    DISCOVERED_GROUPS_SCHEMA_ID,
};

/// kind:39000/39002 events for the wired relay roll into the
/// `"nmp.nip29.discovered_groups"` typed sidecar with the `NDGS` identifier; the
/// payload decodes back into the typed `DiscoveredGroupsSnapshot`, preserving the
/// rolled-up counts and the `Option<String>` fields.
#[test]
fn discovered_groups_typed_sidecar_round_trips() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let app = boot();

    let _handle = open_group_discovery(unsafe { &*app }, HOST.to_string());

    let meta = VerifiedEvent::from_raw_unchecked(raw_event(
        &"1".repeat(64),
        &"f".repeat(64),
        39000,
        100,
        vec![
            vec!["d".into(), "rust-nostr".into()],
            vec!["name".into(), "Rust Nostr".into()],
            vec!["about".into(), "We build NMP.".into()],
        ],
        "",
    ));
    let members = VerifiedEvent::from_raw_unchecked(raw_event(
        &"2".repeat(64),
        &"f".repeat(64),
        39002,
        101,
        vec![
            vec!["d".into(), "rust-nostr".into()],
            vec!["p".into(), "x".repeat(64)],
            vec!["p".into(), "y".repeat(64)],
        ],
        "",
    ));
    inject(app, vec![meta, members]);

    let entry = wait_for_typed("nmp.nip29.discovered_groups", |t| {
        decode_discovered_groups_snapshot(&t.payload)
            .map(|s| s.groups.iter().any(|g| g.member_count == 2))
            .unwrap_or(false)
    })
    .expect("discovered_groups typed sidecar must carry the rolled-up group within 3 s");

    assert_eq!(entry.schema_id, DISCOVERED_GROUPS_SCHEMA_ID);
    assert_eq!(
        entry.file_identifier,
        String::from_utf8_lossy(DISCOVERED_GROUPS_FILE_IDENTIFIER)
    );

    let snapshot = decode_discovered_groups_snapshot(&entry.payload)
        .expect("NDGS payload must decode back into DiscoveredGroupsSnapshot");
    assert_eq!(snapshot.host_relay_url, HOST);
    let group = snapshot
        .groups
        .iter()
        .find(|g| g.group_id == "rust-nostr")
        .expect("decoded snapshot must contain the rolled-up group");
    assert_eq!(group.host_relay_url, HOST);
    assert_eq!(group.name.as_deref(), Some("Rust Nostr"));
    assert_eq!(group.about.as_deref(), Some("We build NMP."));
    // `picture` was never sent → Option round-trips as None (absent string).
    assert_eq!(group.picture, None);
    assert_eq!(group.member_count, 2);
    assert!(group.public && group.open);

    teardown(app);
}

/// Replaceable-event semantics survive the typed encode: a newer 39002 for the
/// same `(kind, d)` supersedes the older one, so the typed payload reflects the
/// later member count.
#[test]
fn discovered_groups_typed_sidecar_reflects_superseding() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let app = boot();

    let _handle = open_group_discovery(unsafe { &*app }, HOST.to_string());

    let older = VerifiedEvent::from_raw_unchecked(raw_event(
        &"3".repeat(64),
        &"f".repeat(64),
        39002,
        100,
        vec![
            vec!["d".into(), "room".into()],
            vec!["p".into(), "a".repeat(64)],
        ],
        "",
    ));
    let newer = VerifiedEvent::from_raw_unchecked(raw_event(
        &"4".repeat(64),
        &"f".repeat(64),
        39002,
        200, // strictly newer created_at → supersedes
        vec![
            vec!["d".into(), "room".into()],
            vec!["p".into(), "a".repeat(64)],
            vec!["p".into(), "b".repeat(64)],
            vec!["p".into(), "c".repeat(64)],
        ],
        "",
    ));
    inject(app, vec![older, newer]);

    let entry = wait_for_typed("nmp.nip29.discovered_groups", |t| {
        decode_discovered_groups_snapshot(&t.payload)
            .map(|s| {
                s.groups
                    .iter()
                    .any(|g| g.group_id == "room" && g.member_count == 3)
            })
            .unwrap_or(false)
    })
    .expect("typed sidecar must reflect the superseding 39002 (member_count == 3) within 3 s");

    let snapshot = decode_discovered_groups_snapshot(&entry.payload).expect("NDGS decode");
    let group = snapshot
        .groups
        .iter()
        .find(|g| g.group_id == "room")
        .expect("group present");
    assert_eq!(
        group.member_count, 3,
        "newer 39002 (3 members) must win over the older (1 member)"
    );

    teardown(app);
}
