//! Wave A proof (codec layer): the `nmp.marmot.messages` typed projection
//! flattens the per-group message map to a sorted `MarmotGroupMessages` vector
//! and round-trips it through the generated `NMMG` bindings.
//!
//! The trait→sidecar surface is proven generically by
//! `nmp-ffi::snapshot::typed_projection_registered_through_trait_surfaces_in_sidecar`
//! (the inherent `NmpApp::register_typed_snapshot_projection` marmot uses
//! delegates to the same registry); this in-crate test proves the
//! marmot-specific schema identity, the map→sorted-vector flattening, and the
//! encode/decode round-trip — including a populated case driven through the
//! real `MarmotProjection` against an in-memory `MarmotService` (no MLS
//! cross-client setup), mirroring `crate::ffi::tests`.

use super::{
    decode_marmot_messages, encode_marmot_messages, typed_projection, FILE_IDENTIFIER, SCHEMA_ID,
    SCHEMA_VERSION,
};
use crate::projection::payload::MarmotMessageRow;
use crate::projection::state::MarmotProjection;
use crate::service::MarmotService;
use mdk_sqlite_storage::MdkSqliteStorage;
use nostr::Keys;
use serde_json::json;

fn in_memory(keys: Keys) -> MarmotService {
    let storage = MdkSqliteStorage::new_in_memory().expect("in-memory mls storage");
    MarmotService::from_storage(storage, keys, Default::default())
}

fn row(id: &str, ts: u64, epoch: Option<u64>) -> MarmotMessageRow {
    MarmotMessageRow {
        id: id.to_string(),
        sender_pubkey_hex: "a".repeat(64),
        content: format!("msg {id}"),
        created_at: ts,
        epoch,
    }
}

#[test]
fn typed_projection_carries_schema_identity_and_round_trips() {
    // Two groups deliberately out of sorted order on input.
    let groups = vec![
        (
            "ff".repeat(32),
            vec![row("m1", 100, Some(3)), row("m2", 101, None)],
        ),
        ("00".repeat(32), vec![row("m3", 200, Some(0))]),
    ];
    let entry = typed_projection(&groups);

    assert_eq!(entry.key, "nmp.marmot.messages");
    assert_eq!(entry.schema_id, SCHEMA_ID);
    assert_eq!(entry.schema_id, "nmp.marmot.messages");
    assert_eq!(entry.schema_version, SCHEMA_VERSION);
    assert_eq!(entry.file_identifier, "NMMG");
    assert_eq!(String::from_utf8_lossy(FILE_IDENTIFIER).into_owned(), "NMMG");
    assert!(!entry.payload.is_empty());

    let decoded = decode_marmot_messages(&entry.payload).expect("must decode as NMMG");
    // Sorted by group_id_hex ascending regardless of input order.
    assert_eq!(decoded.len(), 2);
    assert_eq!(decoded[0].0, "00".repeat(32));
    assert_eq!(decoded[1].0, "ff".repeat(32));
    // First group's two rows preserved in input order.
    assert_eq!(decoded[1].1.len(), 2);
    assert_eq!(decoded[1].1[0].id, "m1");
    assert_eq!(decoded[1].1[0].epoch, Some(3));
    // Option<u64> epoch: None stays None, Some(0) stays Some(0).
    assert_eq!(decoded[1].1[1].epoch, None);
    assert_eq!(decoded[0].1[0].epoch, Some(0));
}

#[test]
fn empty_map_round_trips() {
    let decoded =
        decode_marmot_messages(&encode_marmot_messages(&[])).expect("empty map must decode");
    assert!(decoded.is_empty());
}

#[test]
fn decode_rejects_bytes_without_the_nmmg_identifier() {
    assert!(decode_marmot_messages(b"not a flatbuffer").is_err());
    assert!(decode_marmot_messages(&[]).is_err());
}

/// End-to-end over the real projection / ops code paths: publish → create_group
/// → send, then `messages_all_groups()` (the typed sidecar's source) → encode →
/// decode must surface the sent message. Mirrors
/// `crate::ffi::tests::round_trip_publish_create_snapshot_send_messages`, using
/// an in-memory `MarmotService` (no cross-client MLS).
#[test]
fn messages_all_groups_typed_round_trip_over_real_projection() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let bob = in_memory(bob_keys.clone());
    let bob_kp_json = {
        use nostr::JsonUtil;
        bob.publish_key_package(vec![nostr::RelayUrl::parse("wss://t.relay").unwrap()])
            .expect("bob kp")
            .event_30443
            .as_json()
    };

    let proj = MarmotProjection::new(in_memory(alice_keys.clone()), None);
    proj.with_inner(|h| {
        crate::projection::ops::dispatch(
            h,
            &json!({ "op": "publish_key_package", "relays": ["wss://t.relay"] }),
            1_000,
                None,
        )
    })
    .unwrap();

    let group_id_hex = proj
        .with_inner(|h| {
            crate::projection::ops::dispatch(
                h,
                &json!({
                    "op": "create_group",
                    "name": "Marmot Wire Test",
                    "relays": ["wss://t.relay"],
                    "invitee_npubs": [bob_keys.public_key().to_hex()],
                    "signed_key_package_events_json": [bob_kp_json],
                }),
                1_001,
                        None,
            )
        })
        .unwrap()["group_id_hex"]
        .as_str()
        .unwrap()
        .to_string();

    proj.with_inner(|h| {
        crate::projection::ops::dispatch(
            h,
            &json!({ "op": "send", "group_id_hex": group_id_hex, "text": "hello marmot" }),
            1_003,
                None,
        )
    })
    .unwrap();

    // The typed projection's source method, encoded and decoded.
    let groups = proj.messages_all_groups(200);
    let decoded = decode_marmot_messages(&typed_projection(&groups).payload)
        .expect("real-projection messages must round-trip as NMMG");

    let entry = decoded
        .iter()
        .find(|(gid, _)| gid == &group_id_hex)
        .expect("the created group must appear in the typed messages map");
    assert_eq!(entry.1.len(), 1, "one sent message: {:?}", entry.1);
    assert_eq!(entry.1[0].content, "hello marmot");
    assert_eq!(entry.1[0].sender_pubkey_hex, alice_keys.public_key().to_hex());
}
