//! NIP-29 raw group-events typed-projection sidecar proof.

mod common;

use common::{boot, inject, raw_event, teardown, wait_for_typed, HOST, SERIAL};

use nmp_core::store::VerifiedEvent;
use nmp_nip29::group_id::GroupId;
use nmp_nip29::register::wire_group_events;
use nmp_nip29::{
    decode_group_events_snapshot, GROUP_EVENTS_FILE_IDENTIFIER, GROUP_EVENTS_SCHEMA_ID,
};

#[test]
fn group_events_typed_sidecar_round_trips_raw_tags() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let app = boot();

    wire_group_events(unsafe { &*app }, GroupId::new(HOST, "raw-room"));

    let target = VerifiedEvent::from_raw_unchecked(raw_event(
        &"c".repeat(64),
        &"d".repeat(64),
        16,
        1_700_000_111,
        vec![
            vec!["h".into(), "raw-room".into()],
            vec!["e".into(), "target".into(), HOST.into(), "mention".into()],
            vec!["t".into(), "nostr".into()],
        ],
        "embedded repost",
    ));
    inject(app, vec![target]);

    let entry = wait_for_typed("nmp.nip29.group_events", |t| {
        decode_group_events_snapshot(&t.payload)
            .map(|s| {
                s.events
                    .iter()
                    .any(|e| e.kind == 16 && e.content == "embedded repost")
            })
            .unwrap_or(false)
    })
    .expect("group_events typed sidecar must carry the injected row within 3 s");

    assert_eq!(entry.schema_id, GROUP_EVENTS_SCHEMA_ID);
    assert_eq!(
        entry.file_identifier,
        String::from_utf8_lossy(GROUP_EVENTS_FILE_IDENTIFIER)
    );

    let snapshot = decode_group_events_snapshot(&entry.payload).expect("NGES payload must decode");
    assert_eq!(snapshot.group_id, "raw-room");
    assert_eq!(snapshot.host_relay_url, HOST);
    let row = snapshot
        .events
        .iter()
        .find(|event| event.kind == 16)
        .expect("decoded snapshot must contain the group event");
    assert_eq!(row.id, "c".repeat(64));
    assert_eq!(row.pubkey, "d".repeat(64));
    assert_eq!(row.created_at, 1_700_000_111);
    assert_eq!(
        row.tags,
        vec![
            vec!["h".to_string(), "raw-room".to_string()],
            vec![
                "e".to_string(),
                "target".to_string(),
                HOST.to_string(),
                "mention".to_string()
            ],
            vec!["t".to_string(), "nostr".to_string()],
        ]
    );

    teardown(app);
}

#[test]
fn group_events_typed_sidecar_excludes_other_groups() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let app = boot();

    wire_group_events(unsafe { &*app }, GroupId::new(HOST, "target-room"));

    inject(
        app,
        vec![VerifiedEvent::from_raw_unchecked(raw_event(
            &"e".repeat(64),
            &"f".repeat(64),
            11,
            1_700_000_222,
            vec![vec!["h".into(), "other-room".into()]],
            "wrong room",
        ))],
    );

    let entry = wait_for_typed("nmp.nip29.group_events", |t| {
        decode_group_events_snapshot(&t.payload)
            .map(|s| s.events.is_empty())
            .unwrap_or(false)
    })
    .expect("empty group_events typed sidecar must appear within 3 s");

    let snapshot =
        decode_group_events_snapshot(&entry.payload).expect("empty NGES payload must decode");
    assert!(snapshot.events.is_empty());

    teardown(app);
}
