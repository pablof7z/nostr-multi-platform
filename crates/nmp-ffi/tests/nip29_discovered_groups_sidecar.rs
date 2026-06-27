//! NIP-29 discovered-groups typed-projection sidecar proof.
//!
//! Proves `NmpApp::open_group_discovery` (#2088) emits a typed FlatBuffers
//! sidecar (ADR-0037, `NDGS`) under `"nmp.nip29.discovered_groups"`. Drives the
//! full FFI snapshot path, decodes the frame with
//! `decode_snapshot_typed_projections`, and asserts the typed payload bytes land
//! in the `typed_projections` sidecar, round-tripping back through the generated
//! bindings. (Live-ingest path: events injected AFTER open.)

mod common;

use common::{boot, inject, raw_event, teardown, wait_for_typed, HOST, SERIAL};

use nmp_nip29::{
    decode_discovered_groups_snapshot, DISCOVERED_GROUPS_FILE_IDENTIFIER,
    DISCOVERED_GROUPS_SCHEMA_ID,
};
use nmp_store::VerifiedEvent;

/// kind:39000/39002 events for the wired relay roll into the
/// `"nmp.nip29.discovered_groups"` typed sidecar with the `NDGS` identifier; the
/// payload decodes back into the typed `DiscoveredGroupsSnapshot`, preserving the
/// rolled-up counts and the `Option<String>` fields.
#[test]
fn discovered_groups_typed_sidecar_round_trips() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let app = boot();

    let _handle = unsafe { (*app).open_group_discovery(HOST.to_string()) };

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

    let _handle = unsafe { (*app).open_group_discovery(HOST.to_string()) };

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

/// NIP-29 subgroups (nips PR #2319): `["parent", _]` and `["child", _]` tags on
/// a kind:39000 travel through the typed `NDGS` sidecar, preserving the parent
/// pointer and the ordered child list so a host can render the hierarchy.
#[test]
fn discovered_groups_typed_sidecar_carries_subgroup_tags() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let app = boot();

    let _handle = unsafe { (*app).open_group_discovery(HOST.to_string()) };

    // Parent "tech" with two children, and the child "nostr" pointing back at
    // "tech" — mirrors the spec's tree example.
    let parent = VerifiedEvent::from_raw_unchecked(raw_event(
        &"5".repeat(64),
        &"f".repeat(64),
        39000,
        100,
        vec![
            vec!["d".into(), "tech".into()],
            vec!["name".into(), "Tech".into()],
            vec!["child".into(), "nostr".into()],
            vec!["child".into(), "bitcoin".into()],
        ],
        "",
    ));
    let child = VerifiedEvent::from_raw_unchecked(raw_event(
        &"6".repeat(64),
        &"f".repeat(64),
        39000,
        101,
        vec![
            vec!["d".into(), "nostr".into()],
            vec!["name".into(), "Nostr".into()],
            vec!["parent".into(), "tech".into()],
        ],
        "",
    ));
    inject(app, vec![parent, child]);

    let entry = wait_for_typed("nmp.nip29.discovered_groups", |t| {
        decode_discovered_groups_snapshot(&t.payload)
            .map(|s| {
                s.groups
                    .iter()
                    .any(|g| g.group_id == "nostr" && g.parent.as_deref() == Some("tech"))
            })
            .unwrap_or(false)
    })
    .expect("typed sidecar must carry the subgroup parent within 3 s");

    let snapshot = decode_discovered_groups_snapshot(&entry.payload).expect("NDGS decode");
    let tech = snapshot
        .groups
        .iter()
        .find(|g| g.group_id == "tech")
        .expect("parent group present");
    assert!(
        tech.parent.is_none(),
        "tech is a root group — no parent tag"
    );
    assert_eq!(
        tech.children,
        vec!["nostr".to_string(), "bitcoin".to_string()],
        "child list preserves tag order through the typed sidecar"
    );
    let nostr = snapshot
        .groups
        .iter()
        .find(|g| g.group_id == "nostr")
        .expect("child group present");
    assert_eq!(nostr.parent.as_deref(), Some("tech"));
    assert!(
        nostr.children.is_empty(),
        "nostr has no children declared in this snapshot"
    );

    teardown(app);
}

#[test]
fn discovery_reader_is_the_canonical_sidecar_projection() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let app = boot();

    let (_handle, reader) = unsafe { (*app).open_group_discovery_with_reader(HOST.to_string()) };

    let meta = VerifiedEvent::from_raw_unchecked(raw_event(
        &"7".repeat(64),
        &"f".repeat(64),
        39000,
        100,
        vec![
            vec!["d".into(), "reader-room".into()],
            vec!["name".into(), "Reader Room".into()],
        ],
        "",
    ));
    inject(app, vec![meta]);

    let entry = wait_for_typed("nmp.nip29.discovered_groups", |t| {
        decode_discovered_groups_snapshot(&t.payload)
            .map(|s| s.groups.iter().any(|g| g.group_id == "reader-room"))
            .unwrap_or(false)
    })
    .expect("canonical discovery sidecar must carry the reader-room row within 3 s");

    let sidecar = decode_discovered_groups_snapshot(&entry.payload).expect("NDGS decode");
    assert_eq!(
        reader.snapshot(),
        sidecar,
        "the Rust reader must expose the same projection instance that feeds the typed sidecar"
    );

    teardown(app);
}
