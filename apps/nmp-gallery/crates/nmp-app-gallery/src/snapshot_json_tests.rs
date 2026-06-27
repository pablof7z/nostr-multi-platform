use super::*;
use nmp_core::refs::{encode_ref_row_delta_batch, RefRow, RefRowDeltaBatch};
use nmp_core::typed_projections::{encode_claimed_events, encode_profile, ClaimedEventsModel};
use nmp_core::{encode_snapshot_frame, SnapshotEnvelope, TypedProjectionData};

#[test]
fn empty_typed_snapshot_decodes_to_gallery_shape() {
    let frame = encode_snapshot_frame(
        &SnapshotEnvelope {
            running: true,
            update_kind: "ViewBatch".to_string(),
            ..Default::default()
        },
        &[],
    );

    let mut profiles = RefProfileStore::new();
    let mut events = RefEventStore::new();
    let value: Value = serde_json::from_str(
        &snapshot_json_from_update_frame(&frame, &mut profiles, &mut events).expect("decode"),
    )
    .expect("json");

    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["running"], true);
    assert_eq!(value["projections"][REFS_PROFILE_KEY], json!({}));
    assert_eq!(value["projections"][REFS_EVENT_KEY], json!({}));
    assert_eq!(value["projections"]["accounts"], json!([]));
    assert_eq!(value["projections"]["relay_role_options"], json!([]));
    assert!(value["projections"].get("claimed_events").is_none());
    assert!(value["projections"].get("claimed_event_embeds").is_none());
    assert!(value["projections"].get("signer_state").is_none());
}

#[test]
fn refs_profile_row_delta_surfaces_in_refs_profile_json() {
    // ADR-0063 (#1671): a `refs.profile` baseline row carrying a fresh KPRF
    // card must merge into the store and surface under the
    // `refs.profile` JSON key the native shells read.
    let pubkey = "1111111111111111111111111111111111111111111111111111111111111111";
    let card_payload = encode_profile(&ProfileCardModel {
        pubkey: pubkey.to_string(),
        display_name: Some("Refs Name".to_string()),
        picture_url: Some("https://example.com/refs.png".to_string()),
        ..Default::default()
    });
    let batch = encode_ref_row_delta_batch(&RefRowDeltaBatch {
        namespace: "profile".to_string(),
        baseline: true,
        rows: vec![RefRow::changed(pubkey, 1, card_payload)],
    });
    let frame = encode_snapshot_frame(
        &SnapshotEnvelope {
            running: true,
            update_kind: "ViewBatch".to_string(),
            session_id: 1,
            ..Default::default()
        },
        &[TypedProjectionData {
            key: REFS_PROFILE_KEY.to_string(),
            schema_id: REFS_PROFILE_KEY.to_string(),
            schema_version: 1,
            file_identifier: String::new(),
            payload: batch,
            ..Default::default()
        }],
    );

    let mut profiles = RefProfileStore::new();
    let mut events = RefEventStore::new();
    let value: Value = serde_json::from_str(
        &snapshot_json_from_update_frame(&frame, &mut profiles, &mut events).expect("decode"),
    )
    .expect("json");

    let entry = &value["projections"][REFS_PROFILE_KEY][pubkey];
    assert_eq!(entry["display_name"], "Refs Name");
    assert_eq!(entry["picture_url"], "https://example.com/refs.png");
    assert_eq!(entry["pubkey"], pubkey);
}

#[test]
fn refs_profile_clear_drops_row_from_refs_profile_json() {
    // ADR-0063 (#1671): snapshot_json materialises the FULL current
    // RefProfileStore set each frame. A subsequent `refs.profile` CLEAR
    // (release-on-scroll-off) must DROP the row from the `refs.profile`
    // JSON map — the materialised set is the sole source of truth (D4),
    // no stale row.
    let pubkey = "2222222222222222222222222222222222222222222222222222222222222222";
    let card_payload = encode_profile(&ProfileCardModel {
        pubkey: pubkey.to_string(),
        display_name: Some("Soon Gone".to_string()),
        ..Default::default()
    });

    let mut profiles = RefProfileStore::new();
    let mut events = RefEventStore::new();

    // Frame 1: baseline carrying the resolved card — present.
    let add_frame = encode_snapshot_frame(
        &SnapshotEnvelope {
            running: true,
            update_kind: "ViewBatch".to_string(),
            session_id: 1,
            ..Default::default()
        },
        &[TypedProjectionData {
            key: REFS_PROFILE_KEY.to_string(),
            schema_id: REFS_PROFILE_KEY.to_string(),
            schema_version: 1,
            file_identifier: String::new(),
            payload: encode_ref_row_delta_batch(&RefRowDeltaBatch {
                namespace: "profile".to_string(),
                baseline: true,
                rows: vec![RefRow::changed(pubkey, 1, card_payload)],
            }),
            ..Default::default()
        }],
    );
    let added: Value = serde_json::from_str(
        &snapshot_json_from_update_frame(&add_frame, &mut profiles, &mut events)
            .expect("decode add"),
    )
    .expect("json");
    assert_eq!(
        added["projections"][REFS_PROFILE_KEY][pubkey]["display_name"], "Soon Gone",
        "row must be present after the baseline add"
    );

    // Frame 2: a CLEAR row-delta (release) for the same key — the row must
    // be GONE from the materialised set, not retained as stale.
    let clear_frame = encode_snapshot_frame(
        &SnapshotEnvelope {
            running: true,
            update_kind: "ViewBatch".to_string(),
            session_id: 1,
            ..Default::default()
        },
        &[TypedProjectionData {
            key: REFS_PROFILE_KEY.to_string(),
            schema_id: REFS_PROFILE_KEY.to_string(),
            schema_version: 1,
            file_identifier: String::new(),
            payload: encode_ref_row_delta_batch(&RefRowDeltaBatch {
                namespace: "profile".to_string(),
                baseline: false,
                rows: vec![RefRow::cleared(pubkey, 2)],
            }),
            ..Default::default()
        }],
    );
    let cleared: Value = serde_json::from_str(
        &snapshot_json_from_update_frame(&clear_frame, &mut profiles, &mut events)
            .expect("decode clear"),
    )
    .expect("json");
    assert!(
        cleared["projections"][REFS_PROFILE_KEY]
            .get(pubkey)
            .is_none(),
        "a refs.profile CLEAR must drop the row from the refs.profile map; got {:?}",
        cleared["projections"][REFS_PROFILE_KEY]
    );
}

#[test]
fn refs_event_row_delta_surfaces_resolved_envelope_in_refs_event_json() {
    let primary_id = "3333333333333333333333333333333333333333333333333333333333333333";
    let row = ClaimedEventRow {
        primary_id: primary_id.to_string(),
        id: primary_id.to_string(),
        author_pubkey: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .to_string(),
        kind: 1,
        created_at: 1234,
        tags: Vec::new(),
        content: "hello from refs.event".to_string(),
        content_tree_bytes: Vec::new(),
        signed_event_json: None,
    };
    let row_payload = encode_claimed_events(&ClaimedEventsModel {
        entries: vec![(primary_id.to_string(), row)],
    });
    let frame = encode_snapshot_frame(
        &SnapshotEnvelope {
            running: true,
            update_kind: "ViewBatch".to_string(),
            session_id: 1,
            ..Default::default()
        },
        &[TypedProjectionData {
            key: REFS_EVENT_KEY.to_string(),
            schema_id: REFS_EVENT_KEY.to_string(),
            schema_version: 1,
            file_identifier: String::new(),
            payload: encode_ref_row_delta_batch(&RefRowDeltaBatch {
                namespace: "event".to_string(),
                baseline: true,
                rows: vec![RefRow::changed(primary_id, 1, row_payload)],
            }),
            ..Default::default()
        }],
    );

    let mut profiles = RefProfileStore::new();
    let mut events = RefEventStore::new();
    let value: Value = serde_json::from_str(
        &snapshot_json_from_update_frame(&frame, &mut profiles, &mut events).expect("decode"),
    )
    .expect("json");

    let entry = &value["projections"][REFS_EVENT_KEY][primary_id];
    assert_eq!(entry["primary_id"], primary_id);
    assert_eq!(entry["projection"]["variant"], "shortNote");
    assert_eq!(entry["projection"]["data"]["id"], primary_id);
    assert!(
        entry["projection"]["data"]
            .get("contentTree")
            .and_then(Value::as_object)
            .is_some(),
        "short-note envelopes must carry Rust-tokenized contentTree data; got {entry:?}"
    );
    assert!(value["projections"].get("claimed_event_embeds").is_none());
}

#[test]
fn refs_event_clear_drops_envelope_from_refs_event_json() {
    let primary_id = "4444444444444444444444444444444444444444444444444444444444444444";
    let row = ClaimedEventRow {
        primary_id: primary_id.to_string(),
        id: primary_id.to_string(),
        author_pubkey: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
            .to_string(),
        kind: 1,
        created_at: 1234,
        tags: Vec::new(),
        content: "soon released".to_string(),
        content_tree_bytes: Vec::new(),
        signed_event_json: None,
    };
    let row_payload = encode_claimed_events(&ClaimedEventsModel {
        entries: vec![(primary_id.to_string(), row)],
    });

    let mut profiles = RefProfileStore::new();
    let mut events = RefEventStore::new();
    let add_frame = encode_snapshot_frame(
        &SnapshotEnvelope {
            running: true,
            update_kind: "ViewBatch".to_string(),
            session_id: 1,
            ..Default::default()
        },
        &[TypedProjectionData {
            key: REFS_EVENT_KEY.to_string(),
            schema_id: REFS_EVENT_KEY.to_string(),
            schema_version: 1,
            file_identifier: String::new(),
            payload: encode_ref_row_delta_batch(&RefRowDeltaBatch {
                namespace: "event".to_string(),
                baseline: true,
                rows: vec![RefRow::changed(primary_id, 1, row_payload)],
            }),
            ..Default::default()
        }],
    );
    let added: Value = serde_json::from_str(
        &snapshot_json_from_update_frame(&add_frame, &mut profiles, &mut events)
            .expect("decode add"),
    )
    .expect("json");
    assert_eq!(
        added["projections"][REFS_EVENT_KEY][primary_id]["projection"]["variant"],
        "shortNote"
    );

    let clear_frame = encode_snapshot_frame(
        &SnapshotEnvelope {
            running: true,
            update_kind: "ViewBatch".to_string(),
            session_id: 1,
            ..Default::default()
        },
        &[TypedProjectionData {
            key: REFS_EVENT_KEY.to_string(),
            schema_id: REFS_EVENT_KEY.to_string(),
            schema_version: 1,
            file_identifier: String::new(),
            payload: encode_ref_row_delta_batch(&RefRowDeltaBatch {
                namespace: "event".to_string(),
                baseline: false,
                rows: vec![RefRow::cleared(primary_id, 2)],
            }),
            ..Default::default()
        }],
    );
    let cleared: Value = serde_json::from_str(
        &snapshot_json_from_update_frame(&clear_frame, &mut profiles, &mut events)
            .expect("decode clear"),
    )
    .expect("json");
    assert!(
        cleared["projections"][REFS_EVENT_KEY]
            .get(primary_id)
            .is_none(),
        "a refs.event CLEAR must drop the envelope from the refs.event map; got {:?}",
        cleared["projections"][REFS_EVENT_KEY]
    );
}

#[test]
fn profile_card_json_adds_gallery_display_fields() {
    let card = ProfileCardModel {
        pubkey: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        display_name: Some("Alice".to_string()),
        about: String::new(),
        picture_url: None,
        nip05: String::new(),
        lnurl: None,
        ..Default::default()
    };

    let value = profile_card_json(&card, &card.pubkey);

    assert_eq!(value["pubkey"], card.pubkey);
    assert_eq!(value["display_name"], "Alice");
    assert!(value["npub"].as_str().unwrap_or("").starts_with("npub1"));
    assert!(value["npub_short"]
        .as_str()
        .unwrap_or("")
        .starts_with("npub1"));
    assert!(value["about"].is_null());
    assert!(value["nip05"].is_null());
}
