//! NIP-29 joined-groups typed-projection sidecar proof.

mod common;

use common::{boot, inject, raw_event, teardown, wait_for_typed, HOST, SERIAL};

use nmp_core::store::VerifiedEvent;
use nmp_nip29::register::wire_joined_groups;
use nmp_nip29::{
    decode_joined_groups_snapshot, JOINED_GROUPS_FILE_IDENTIFIER, JOINED_GROUPS_SCHEMA_ID,
};

#[test]
fn joined_groups_typed_sidecar_round_trips_membership_and_admin_status() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let app = boot();
    let active = "a".repeat(64);

    wire_joined_groups(unsafe { &*app }, active.clone(), HOST.to_string());

    let meta = VerifiedEvent::from_raw_unchecked(raw_event(
        &"1".repeat(64),
        &"f".repeat(64),
        39000,
        100,
        vec![
            vec!["d".into(), "rust-nostr".into()],
            vec!["name".into(), "Rust Nostr".into()],
            vec!["closed".into()],
        ],
        "",
    ));
    let admins = VerifiedEvent::from_raw_unchecked(raw_event(
        &"2".repeat(64),
        &"f".repeat(64),
        39001,
        101,
        vec![
            vec!["d".into(), "rust-nostr".into()],
            vec!["p".into(), active.clone()],
        ],
        "",
    ));
    let members = VerifiedEvent::from_raw_unchecked(raw_event(
        &"3".repeat(64),
        &"f".repeat(64),
        39002,
        102,
        vec![
            vec!["d".into(), "rust-nostr".into()],
            vec!["p".into(), active.clone()],
            vec!["p".into(), "b".repeat(64)],
        ],
        "",
    ));
    let other_group = VerifiedEvent::from_raw_unchecked(raw_event(
        &"4".repeat(64),
        &"f".repeat(64),
        39002,
        102,
        vec![
            vec!["d".into(), "elsewhere".into()],
            vec!["p".into(), "c".repeat(64)],
        ],
        "",
    ));
    inject(app, vec![meta, admins, members, other_group]);

    let entry = wait_for_typed("nmp.nip29.joined_groups", |t| {
        decode_joined_groups_snapshot(&t.payload)
            .map(|s| {
                s.groups
                    .iter()
                    .any(|g| g.group_id == "rust-nostr" && g.is_admin)
            })
            .unwrap_or(false)
    })
    .expect("joined_groups typed sidecar must carry the active user's group within 3 s");

    assert_eq!(entry.schema_id, JOINED_GROUPS_SCHEMA_ID);
    assert_eq!(
        entry.file_identifier,
        String::from_utf8_lossy(JOINED_GROUPS_FILE_IDENTIFIER)
    );

    let snapshot = decode_joined_groups_snapshot(&entry.payload).expect("NJGS payload must decode");
    assert_eq!(snapshot.active_pubkey, active);
    assert_eq!(snapshot.groups.len(), 1);
    let group = &snapshot.groups[0];
    assert_eq!(group.group_id, "rust-nostr");
    assert_eq!(group.host_relay_url, HOST);
    assert_eq!(group.name.as_deref(), Some("Rust Nostr"));
    assert_eq!(group.member_count, 2);
    assert_eq!(group.admin_count, 1);
    assert!(group.is_member);
    assert!(group.is_admin);
    assert!(group.public);
    assert!(!group.open);

    teardown(app);
}

#[test]
fn joined_groups_sidecar_reflects_latest_relay_snapshot_only() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let app = boot();
    let active = "a".repeat(64);

    wire_joined_groups(unsafe { &*app }, active.clone(), HOST.to_string());

    let add_request = VerifiedEvent::from_raw_unchecked(raw_event(
        &"5".repeat(64),
        &"f".repeat(64),
        9000,
        100,
        vec![
            vec!["h".into(), "room".into()],
            vec!["p".into(), active.clone()],
        ],
        "",
    ));
    let older_members = VerifiedEvent::from_raw_unchecked(raw_event(
        &"6".repeat(64),
        &"f".repeat(64),
        39002,
        101,
        vec![
            vec!["d".into(), "room".into()],
            vec!["p".into(), active.clone()],
        ],
        "",
    ));
    let newer_members = VerifiedEvent::from_raw_unchecked(raw_event(
        &"7".repeat(64),
        &"f".repeat(64),
        39002,
        200,
        vec![
            vec!["d".into(), "room".into()],
            vec!["p".into(), "b".repeat(64)],
        ],
        "",
    ));
    inject(app, vec![add_request, older_members, newer_members]);

    let entry = wait_for_typed("nmp.nip29.joined_groups", |t| {
        decode_joined_groups_snapshot(&t.payload)
            .map(|s| s.groups.is_empty())
            .unwrap_or(false)
    })
    .expect("latest relay-signed 39002 must remove the joined row within 3 s");

    let snapshot = decode_joined_groups_snapshot(&entry.payload).expect("NJGS payload must decode");
    assert!(snapshot.groups.is_empty());

    teardown(app);
}
