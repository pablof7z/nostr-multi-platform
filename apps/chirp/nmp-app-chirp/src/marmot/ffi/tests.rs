//! Marmot FFI smoke tests.
//!
//! Mirrors `crate::ffi::tests` (null-pointer D6 + a round-trip), and the
//! two-party in-memory pattern from `crates/nmp-marmot/src/tests.rs`:
//! register-equivalent → publish_key_package → create_group (seeded via
//! the `signed_key_package_events_json` KeyPackage-cache-seam escape
//! hatch with a second in-memory service's KeyPackage) → snapshot reflects
//! the group → send → group_messages returns it.
//!
//! The on-disk `nmp_app_chirp_marmot_register` path needs a keyring +
//! SQLite file, so the round-trip drives the SAME code the FFI symbols
//! invoke (`MarmotProjection::snapshot`, `with_inner(ops::dispatch)`,
//! `ops::group_messages`) against an in-memory `MarmotService`. The
//! C-ABI symbols themselves are covered by the null-pointer / lifetime
//! tests below.

use super::*;
use crate::marmot::{ops, state::MarmotProjection};

use mdk_sqlite_storage::MdkSqliteStorage;
use nmp_marmot::service::MarmotService;
use nostr::Keys;
use serde_json::json;

fn in_memory(keys: Keys) -> MarmotService {
    let storage = MdkSqliteStorage::new_in_memory().expect("in-memory mls storage");
    MarmotService::from_storage(storage, keys, Default::default())
}

// ── C-ABI D6 / lifetime ──────────────────────────────────────────────────

#[test]
fn null_pointer_paths_are_silent() {
    assert!(nmp_app_chirp_marmot_register(
        std::ptr::null_mut(),
        std::ptr::null(),
        std::ptr::null()
    )
    .is_null());
    assert!(nmp_app_chirp_marmot_snapshot(std::ptr::null_mut()).is_null());
    assert!(
        nmp_app_chirp_marmot_group_messages(std::ptr::null_mut(), std::ptr::null())
            .is_null()
    );
    assert!(nmp_app_chirp_marmot_dispatch(std::ptr::null_mut(), std::ptr::null()).is_null());
    nmp_app_chirp_marmot_string_free(std::ptr::null_mut());
    nmp_app_chirp_marmot_unregister(std::ptr::null_mut());
}

#[test]
fn register_with_null_app_returns_null() {
    let h = nmp_app_chirp_marmot_register(
        std::ptr::null_mut(),
        std::ptr::null(),
        std::ptr::null(),
    );
    assert!(h.is_null());
}

// ── Round-trip over the real projection / ops code paths ─────────────────

#[test]
fn round_trip_publish_create_snapshot_send_messages() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    // Bob: a second service used only to mint a KeyPackage to invite.
    let bob = in_memory(bob_keys.clone());
    let bob_kp = bob
        .publish_key_package(vec![nostr::RelayUrl::parse("wss://t.relay").unwrap()])
        .expect("bob kp");
    let bob_kp_json = {
        use nostr::JsonUtil;
        bob_kp.event_30443.as_json()
    };

    // Alice: the projection the FFI symbols drive.
    let proj = MarmotProjection::new(in_memory(alice_keys.clone()));

    // 1. publish_key_package dispatch.
    let r = proj
        .with_inner(|h| {
            ops::dispatch(
                h,
                &json!({ "op": "publish_key_package",
                         "relays": ["wss://t.relay"] }),
                1_000,
            )
        })
        .unwrap();
    assert_eq!(r["ok"], json!(true), "publish_key_package: {r}");
    assert!(r["events"].as_array().unwrap().len() == 2);

    // Snapshot now shows key_package.published == true.
    let snap = proj.snapshot(1_000);
    assert!(snap.key_package.published);
    assert_eq!(snap.key_package.age_secs, Some(0));
    assert!(!snap.key_package.stale);
    assert!(snap.groups.is_empty());

    // 2. create_group dispatch (seeded via the KeyPackage-cache seam).
    let r = proj
        .with_inner(|h| {
            ops::dispatch(
                h,
                &json!({
                    "op": "create_group",
                    "name": "Marmot FFI Test",
                    "description": "round-trip",
                    "relays": ["wss://t.relay"],
                    "invitee_npubs": [bob_keys.public_key().to_hex()],
                    "signed_key_package_events_json": [bob_kp_json],
                }),
                1_001,
            )
        })
        .unwrap();
    assert_eq!(r["ok"], json!(true), "create_group: {r}");
    let group_id_hex = r["group_id_hex"].as_str().unwrap().to_string();
    assert!(!group_id_hex.is_empty());
    assert_eq!(r["welcome_rumors"].as_array().unwrap().len(), 1);

    // 3. snapshot reflects the group (Alice + Bob members).
    let snap = proj.snapshot(1_002);
    assert_eq!(snap.groups.len(), 1, "snapshot groups: {snap:?}");
    let g = &snap.groups[0];
    assert_eq!(g.id_hex, group_id_hex);
    assert_eq!(g.name, "Marmot FFI Test");
    assert_eq!(g.members.len(), 2);
    assert!(g.members.contains(&alice_keys.public_key().to_hex()));
    assert!(g.members.contains(&bob_keys.public_key().to_hex()));
    assert_eq!(g.unread, 0);

    // 4. send dispatch.
    let r = proj
        .with_inner(|h| {
            ops::dispatch(
                h,
                &json!({ "op": "send",
                         "group_id_hex": group_id_hex,
                         "text": "hello marmot" }),
                1_003,
            )
        })
        .unwrap();
    assert_eq!(r["ok"], json!(true), "send: {r}");
    assert!(r["event"].as_str().is_some());

    // 5. group_messages returns the sent message.
    let rows = proj
        .with_inner(|h| ops::group_messages(h, &group_id_hex, 200))
        .unwrap();
    assert_eq!(rows.len(), 1, "group_messages: {rows:?}");
    assert_eq!(rows[0].content, "hello marmot");
    assert_eq!(rows[0].sender_npub, alice_keys.public_key().to_hex());

    // Snapshot now counts the message.
    let snap = proj.snapshot(1_004);
    assert_eq!(snap.groups[0].unread, 1);
}

#[test]
fn create_group_without_key_packages_reports_seam() {
    let proj = MarmotProjection::new(in_memory(Keys::generate()));
    let r = proj
        .with_inner(|h| {
            ops::dispatch(
                h,
                &json!({
                    "op": "create_group",
                    "name": "g",
                    "relays": ["wss://t.relay"],
                    "invitee_npubs": ["abc"],
                }),
                1,
            )
        })
        .unwrap();
    assert_eq!(r["ok"], json!(false));
    assert_eq!(r["error"], json!("key_package_unavailable"));
    assert_eq!(r["needs"], json!(["abc"]));
}

#[test]
fn unknown_op_and_bad_json_degrade() {
    let proj = MarmotProjection::new(in_memory(Keys::generate()));
    let r = proj
        .with_inner(|h| ops::dispatch(h, &json!({ "op": "frobnicate" }), 1))
        .unwrap();
    assert_eq!(r["ok"], json!(false));
    assert!(r["error"].as_str().unwrap().contains("unknown op"));

    let r = proj
        .with_inner(|h| ops::dispatch(h, &json!({ "no_op": true }), 1))
        .unwrap();
    assert_eq!(r["ok"], json!(false));
}
