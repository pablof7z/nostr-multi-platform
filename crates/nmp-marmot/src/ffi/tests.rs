//! Marmot FFI smoke tests.
//!
//! Mirrors `crate::ffi::tests` (null-pointer D6 + a round-trip), and the
//! two-party in-memory pattern from `crates/nmp-marmot/src/tests.rs`:
//! register-equivalent → publish_key_package → create_group (seeded via
//! the `signed_key_package_events_json` KeyPackage-cache-seam escape
//! hatch with a second in-memory service's KeyPackage) → snapshot reflects
//! the group → send → group_messages returns it.
//!
//! The on-disk `register_with_secret_hex` path needs a keyring +
//! SQLite file, so the round-trip drives the SAME code the FFI symbols
//! invoke (`MarmotProjection::snapshot`, `with_inner(ops::dispatch)`,
//! `ops::group_messages`) against an in-memory `MarmotService`. The
//! C-ABI symbols themselves are covered by the null-pointer / lifetime
//! tests below.

use super::*;
use crate::projection::tap::{MARMOT_INGEST_SLOT, MarmotIngestParser};
use crate::projection::{ops, ops::ingest_signed_event_core, state::MarmotProjection};

use crate::service::MarmotService;
use mdk_core::prelude::NostrGroupConfigData;
use mdk_sqlite_storage::MdkSqliteStorage;
use nmp_core::decode_snapshot_typed_projections;
use nmp_core::substrate::IngestParser;
use nmp_core::typed_projections::{ACTION_LIFECYCLE_SCHEMA_ID, decode_action_lifecycle};
use nmp_store::{RawEvent, VerifiedEvent};
use nostr::{JsonUtil, Keys};
use serde_json::json;
use std::ffi::c_void;
use std::ffi::{CStr, CString};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// Parse a gift-wrap JSON string into a `VerifiedEvent` for use with `IngestParser::parse`.
///
/// Performs full Schnorr verification via `VerifiedEvent::try_from_raw`.
/// Panics if the JSON is malformed or the signature does not verify — acceptable in
/// tests where the event was just constructed with real keys.
fn gift_wrap_to_verified(json: &str) -> VerifiedEvent {
    let raw: RawEvent =
        serde_json::from_str(json).expect("gift_wrap_json must deserialize to RawEvent");
    VerifiedEvent::try_from_raw(raw).expect("gift_wrap_json must pass Schnorr verification")
}

fn in_memory(keys: Keys) -> MarmotService {
    let storage = MdkSqliteStorage::new_in_memory().expect("in-memory mls storage");
    MarmotService::from_storage(storage, keys, Default::default())
}

static ACTION_FRAME_CAPTURE_LOCK: Mutex<()> = Mutex::new(());
static ACTION_FRAME_TX: OnceLock<Mutex<Option<Sender<Vec<u8>>>>> = OnceLock::new();

extern "C" fn capture_action_frame(_ctx: *mut c_void, ptr: *const u8, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }
    // SAFETY: nmp-ffi keeps the frame bytes valid for the callback duration;
    // tests copy them immediately and never retain the raw pointer.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
    if let Some(slot) = ACTION_FRAME_TX.get() {
        if let Ok(guard) = slot.lock() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(bytes);
            }
        }
    }
}

fn install_action_frame_capture() -> Receiver<Vec<u8>> {
    let (tx, rx) = channel::<Vec<u8>>();
    let slot = ACTION_FRAME_TX.get_or_init(|| Mutex::new(None));
    *slot.lock().unwrap() = Some(tx);
    rx
}

fn uninstall_action_frame_capture() {
    if let Some(slot) = ACTION_FRAME_TX.get() {
        *slot.lock().unwrap() = None;
    }
}

// ── C-ABI D6 / lifetime ──────────────────────────────────────────────────

#[test]
fn null_pointer_paths_are_silent() {
    // V-107 / ADR-0039: `nmp_marmot_snapshot`, `nmp_marmot_group_messages`,
    // and `nmp_marmot_string_free` were deleted. Their null-pointer D6 cases
    // were verified against the still-exported lifecycle symbols below.
    assert!(
        register_with_secret_hex(
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
        )
        .is_null()
    );
    nmp_marmot_unregister(std::ptr::null_mut());
}

#[test]
fn register_with_null_app_returns_null() {
    let h = register_with_secret_hex(
        std::ptr::null_mut(),
        std::ptr::null(),
        std::ptr::null(),
        std::ptr::null(),
    );
    assert!(h.is_null());
}

// The keyring-aware sign-in policy test (persist → recall → forget through the
// `nmp-marmot::identity` entry points) lives in the sibling
// `keyring_identity_tests` module — split out to keep this file under the
// 1000-LOC hard cap (issue #622).

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
    let proj = MarmotProjection::new(in_memory(alice_keys.clone()), None);

    // 1. publish_key_package dispatch.
    let r = proj
        .with_inner(|h| {
            ops::dispatch_json_for_tests(
                h,
                json!({ "op": "publish_key_package",
                         "relays": ["wss://t.relay"] }),
                1_000,
                None,
            )
        })
        .unwrap();
    assert_eq!(r["ok"], json!(true), "publish_key_package: {r}");
    assert!(r["events"].as_array().unwrap().len() == 1); // kind:443 retired; only kind:30443

    // Snapshot now shows key_package.published == true.
    let snap = proj.snapshot(1_000);
    assert!(snap.key_package.published);
    assert_eq!(snap.key_package.age_secs, Some(0));
    assert!(!snap.key_package.stale);
    assert!(snap.groups.is_empty());

    // 2. create_group dispatch (seeded via the KeyPackage-cache seam).
    let r = proj
        .with_inner(|h| {
            ops::dispatch_json_for_tests(
                h,
                json!({
                    "op": "create_group",
                    "name": "Marmot FFI Test",
                    "description": "round-trip",
                    "relays": ["wss://t.relay"],
                    "invitee_npubs": [bob_keys.public_key().to_hex()],
                    "signed_key_package_events_json": [bob_kp_json],
                }),
                1_001,
                None,
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
    assert_eq!(g.unread_count, None);

    // 4. send dispatch.
    let r = proj
        .with_inner(|h| {
            ops::dispatch_json_for_tests(
                h,
                json!({ "op": "send",
                         "group_id_hex": group_id_hex,
                         "text": "hello marmot" }),
                1_003,
                None,
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
    assert_eq!(rows[0].sender_pubkey_hex, alice_keys.public_key().to_hex());

    // Snapshot now counts the message.
    let snap = proj.snapshot(1_004);
    assert_eq!(snap.groups[0].unread_count, Some(1));
}

#[test]
fn create_group_without_key_packages_reports_seam() {
    let proj = MarmotProjection::new(in_memory(Keys::generate()), None);
    let r = proj
        .with_inner(|h| {
            ops::dispatch_json_for_tests(
                h,
                json!({
                    "op": "create_group",
                    "name": "g",
                    "relays": ["wss://t.relay"],
                    "invitee_npubs": ["abc"],
                }),
                1,
                None,
            )
        })
        .unwrap();
    assert_eq!(r["ok"], json!(false));
    assert_eq!(r["error"], json!("key_package_unavailable"));
    assert_eq!(r["needs"], json!(["abc"]));
    assert_eq!(r["fetch_requested"], json!(0));
    assert_eq!(
        r["hint"],
        json!("key package lookup was requested; results arrive via the kernel tap")
    );
}

#[test]
fn create_group_partial_key_package_set_reports_only_missing_invitees() {
    let bob_keys = Keys::generate();
    let carol_keys = Keys::generate();
    let bob = in_memory(bob_keys.clone());
    let bob_kp_json = bob
        .publish_key_package(vec![nostr::RelayUrl::parse("wss://t.relay").unwrap()])
        .expect("bob key package")
        .event_30443
        .as_json();

    let proj = MarmotProjection::new(in_memory(Keys::generate()), None);
    let r = proj
        .with_inner(|h| {
            ops::dispatch_json_for_tests(
                h,
                json!({
                    "op": "create_group",
                    "name": "g",
                    "relays": ["wss://t.relay"],
                    "invitee_npubs": [
                        bob_keys.public_key().to_hex(),
                        carol_keys.public_key().to_hex()
                    ],
                    "signed_key_package_events_json": [bob_kp_json],
                }),
                1,
                None,
            )
        })
        .unwrap();

    assert_eq!(r["ok"], json!(false));
    assert_eq!(r["error"], json!("key_package_unavailable"));
    assert_eq!(r["needs"], json!([carol_keys.public_key().to_hex()]));
    assert_eq!(r["fetch_requested"], json!(1));
}

#[test]
fn invite_partial_key_package_set_reports_only_missing_invitees() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();
    let carol_keys = Keys::generate();
    let bob = in_memory(bob_keys.clone());
    let bob_kp_json = bob
        .publish_key_package(vec![nostr::RelayUrl::parse("wss://t.relay").unwrap()])
        .expect("bob key package")
        .event_30443
        .as_json();

    let proj = MarmotProjection::new(in_memory(alice_keys), None);
    let group_id_hex = proj
        .with_inner(|h| {
            ops::dispatch_json_for_tests(
                h,
                json!({
                    "op": "create_group",
                    "name": "g",
                    "relays": ["wss://t.relay"],
                }),
                1,
                None,
            )
        })
        .unwrap()["group_id_hex"]
        .as_str()
        .unwrap()
        .to_string();

    let r = proj
        .with_inner(|h| {
            ops::dispatch_json_for_tests(
                h,
                json!({
                    "op": "invite",
                    "group_id_hex": group_id_hex,
                    "invitee_npubs": [
                        bob_keys.public_key().to_hex(),
                        carol_keys.public_key().to_hex()
                    ],
                    "signed_key_package_events_json": [bob_kp_json],
                }),
                2,
                None,
            )
        })
        .unwrap();

    assert_eq!(r["ok"], json!(false));
    assert_eq!(r["error"], json!("key_package_unavailable"));
    assert_eq!(r["needs"], json!([carol_keys.public_key().to_hex()]));
    assert_eq!(r["fetch_requested"], json!(1));
}

#[test]
fn unknown_op_and_bad_json_degrade() {
    let proj = MarmotProjection::new(in_memory(Keys::generate()), None);
    let r = proj
        .with_inner(|h| ops::dispatch_json_for_tests(h, json!({ "op": "frobnicate" }), 1, None))
        .unwrap();
    assert_eq!(r["ok"], json!(false));
    assert!(
        r["error"]
            .as_str()
            .unwrap()
            .contains("invalid MarmotAction")
    );

    let r = proj
        .with_inner(|h| ops::dispatch_json_for_tests(h, json!({ "no_op": true }), 1, None))
        .unwrap();
    assert_eq!(r["ok"], json!(false));
}

mod dispatch_action_tests;
mod ingest_tests;
mod push_projection_tests;
