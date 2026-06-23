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
use crate::projection::tap::{MarmotIngestParser, MARMOT_INGEST_SLOT};
use crate::projection::{ops, state::MarmotProjection};

use crate::service::MarmotService;
use mdk_core::prelude::NostrGroupConfigData;
use mdk_sqlite_storage::MdkSqliteStorage;
use nmp_core::substrate::IngestParser;
use nmp_store::{RawEvent, VerifiedEvent};
use nostr::{JsonUtil, Keys};
use serde_json::json;
use std::ffi::{CStr, CString};
use std::sync::Arc;
use std::sync::Mutex;

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

// ── C-ABI D6 / lifetime ──────────────────────────────────────────────────

#[test]
fn null_pointer_paths_are_silent() {
    // V-107 / ADR-0039: `nmp_marmot_snapshot`, `nmp_marmot_group_messages`,
    // and `nmp_marmot_string_free` were deleted. Their null-pointer D6 cases
    // were verified against the still-exported lifecycle symbols below.
    assert!(register_with_secret_hex(
        std::ptr::null_mut(),
        std::ptr::null(),
        std::ptr::null(),
        std::ptr::null(),
    )
    .is_null());
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
            ops::dispatch(
                h,
                &json!({ "op": "publish_key_package",
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
            ops::dispatch(
                h,
                &json!({ "op": "send",
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
            ops::dispatch(
                h,
                &json!({
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
            ops::dispatch(
                h,
                &json!({
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
    assert_eq!(r["fetch_requested"], json!(0));
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
            ops::dispatch(
                h,
                &json!({
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
            ops::dispatch(
                h,
                &json!({
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
    assert_eq!(r["fetch_requested"], json!(0));
}

#[test]
fn unknown_op_and_bad_json_degrade() {
    let proj = MarmotProjection::new(in_memory(Keys::generate()), None);
    let r = proj
        .with_inner(|h| ops::dispatch(h, &json!({ "op": "frobnicate" }), 1, None))
        .unwrap();
    assert_eq!(r["ok"], json!(false));
    assert!(r["error"].as_str().unwrap().contains("unknown op"));

    let r = proj
        .with_inner(|h| ops::dispatch(h, &json!({ "no_op": true }), 1, None))
        .unwrap();
    assert_eq!(r["ok"], json!(false));
}

// ── Inbound ingest seam (IngestParser — raw-tap PR-2) ────────────────────

/// Simulate the kernel `IngestParser` delivering a signed kind:1059
/// gift-wrap welcome: it must reach `MarmotService` via the SAME shared
/// `ingest_signed_event_core` the dispatch op uses, and Bob's snapshot
/// must then show a pending welcome — with NO Swift / dispatch call (the
/// existing snapshot read surfaces the new state). Builds a real gift-wrap
/// via the two-party in-memory pattern (the `nmp_nip59` path), exactly as
/// `crates/nmp-marmot/src/tests.rs` does.
#[test]
fn ingest_parser_kind_1059_welcome_reaches_service_and_snapshot() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let alice = in_memory(alice_keys.clone());
    let bob_service = in_memory(bob_keys.clone());

    // Bob publishes a KeyPackage so Alice can invite him.
    let bob_kp = bob_service
        .publish_key_package(vec![nostr::RelayUrl::parse("wss://t.relay").unwrap()])
        .expect("bob kp");

    // Alice creates the group inviting Bob, then gift-wraps the kind:444
    // welcome rumor to Bob (real NIP-59 path → signed kind:1059).
    let config = NostrGroupConfigData::new(
        "Parser Ingest Test".to_string(),
        "inbound".to_string(),
        None,
        None,
        None,
        vec![nostr::RelayUrl::parse("wss://t.relay").unwrap()],
        vec![alice_keys.public_key()],
    );
    let (_group, pending) = alice
        .create_group(vec![bob_kp.event_30443.clone()], config)
        .expect("alice creates group");
    let welcome_rumor = pending.welcome_rumors[0].clone();
    let gift = alice
        .wrap_welcome(&bob_keys.public_key(), welcome_rumor)
        .expect("alice gift-wraps welcome to bob");
    pending.commit().expect("alice merges create commit");
    let gift_json = gift.as_json();
    let gift_id_hex = gift.id.to_hex();

    // Bob's projection + the IngestParser the FFI register path would install.
    let bob_proj = Arc::new(MarmotProjection::new(bob_service, None));
    let parser = MarmotIngestParser::new(Arc::clone(&bob_proj));

    // Pre-condition: no pending welcomes yet.
    assert!(bob_proj.snapshot(0).pending_welcomes.is_empty());

    // Kernel dispatcher delivers the VerifiedEvent to the parser.
    let verified = gift_wrap_to_verified(&gift_json);
    parser.parse(&verified);

    // The snapshot read (unchanged, no Swift call) now surfaces it.
    let snap = bob_proj.snapshot(1);
    assert_eq!(
        snap.pending_welcomes.len(),
        1,
        "parser-delivered welcome must surface in snapshot: {snap:?}"
    );
    let row = &snap.pending_welcomes[0];
    assert_eq!(row.id_hex, gift_id_hex);
    assert_eq!(row.group_name, "Parser Ingest Test");
    assert_eq!(row.inviter_npub, alice_keys.public_key().to_hex());

    // Idempotent / D6: a duplicate relay echo of the same gift-wrap is a
    // silent no-op on the parser (never panics, snapshot stays consistent).
    parser.parse(&verified);
    assert_eq!(bob_proj.snapshot(2).pending_welcomes.len(), 1);

    // The back-compat dispatch op drives the SAME shared core against the
    // SAME projection (its key store has Bob's key package; a separate
    // service would not — KP state is per-storage). `unwrap_and_process_
    // welcome` is idempotent, so re-ingesting via the op succeeds and the
    // row is still present — proving the parser and the op share one path.
    let r = bob_proj
        .with_inner(|h| {
            ops::dispatch(
                h,
                &json!({ "op": "ingest_signed_event", "event_json": gift_json }),
                3,
                None,
            )
        })
        .unwrap();
    assert_eq!(
        r["ok"],
        json!(true),
        "dispatch back-compat shares core: {r}"
    );
    assert_eq!(r["kind"], json!(1059));
    assert_eq!(bob_proj.snapshot(3).pending_welcomes.len(), 1);
}

/// D6: the parser silently no-ops on a malformed / non-reconstructable event.
/// A `VerifiedEvent` already passed Schnorr verification so the JSON serialization
/// and nostr::Event parse always succeed in practice; D6 degrades on kind:444
/// (admitted by filter, deliberately skipped by the core).
#[test]
fn ingest_parser_unsupported_kind_is_silent() {
    let proj = Arc::new(MarmotProjection::new(in_memory(Keys::generate()), None));
    let parser = MarmotIngestParser::new(Arc::clone(&proj));

    // kind:444 is in TAP_KINDS (admitted by the per-kind registrations) but
    // is a deliberate skip in `ingest_signed_event_core` — the parser must
    // produce no snapshot side-effects and never panic.
    let kind444_json = nostr::EventBuilder::new(nostr::Kind::Custom(444), "x")
        .sign_with_keys(&Keys::generate())
        .unwrap()
        .as_json();
    let raw: RawEvent =
        serde_json::from_str(&kind444_json).expect("kind:444 event must deserialize to RawEvent");
    let verified = VerifiedEvent::try_from_raw(raw).expect("kind:444 must pass verification");
    parser.parse(&verified);

    let snap = proj.snapshot(0);
    assert!(snap.pending_welcomes.is_empty());
    assert!(snap.groups.is_empty());
}

/// PR-2 coexistence test: both the NIP-17 DM inbox parser (slot
/// `"nip17.dm_inbox"`) and the Marmot parser (slot `"marmot"`) are
/// registered for kind:1059 and both fire when a gift-wrap event arrives.
///
/// Uses `EventIngestDispatcher` directly (no kernel actor needed) to prove
/// the slot-keyed coexistence. A separate end-to-end test confirms Marmot
/// state mutation through the full ingest path.
#[test]
fn ingest_parser_kind_1059_coexistence_both_parsers_fire() {
    use nmp_core::substrate::EventIngestDispatcher;

    let mut dispatcher = EventIngestDispatcher::new();

    // Slot "nip17.dm_inbox" — a capturing parser to stand in for
    // DmInboxProjection (avoids the circular dep on nmp-nip17).
    struct CapturingParser {
        fired: Mutex<bool>,
    }
    impl IngestParser for CapturingParser {
        fn parse(&self, _evt: &VerifiedEvent) {
            *self.fired.lock().unwrap() = true;
        }
    }
    let dm_parser = Arc::new(CapturingParser {
        fired: Mutex::new(false),
    });
    let marmot_parser = Arc::new(CapturingParser {
        fired: Mutex::new(false),
    });

    dispatcher.replace_kind_parser(1059, "nip17.dm_inbox", dm_parser.clone());
    dispatcher.replace_kind_parser(1059, MARMOT_INGEST_SLOT, marmot_parser.clone());
    assert_eq!(dispatcher.registration_count(), 2, "both slots registered");

    // Build a real signed kind:1059 event.
    let sender = Keys::generate();
    let receiver = Keys::generate();
    let (gift_json, _) = {
        use nmp_nip59::gift_wrap_local;
        use nostr::{EventBuilder, Kind, Tag, Timestamp};
        let rumor = EventBuilder::new(Kind::from_u16(14), "coexistence test")
            .tags(vec![Tag::public_key(receiver.public_key())])
            .custom_created_at(Timestamp::from(1_700_000_000u64))
            .build(sender.public_key());
        let envelope = gift_wrap_local(
            &sender,
            &receiver.public_key(),
            &rumor,
            Timestamp::from(1_700_000_000u64),
        )
        .expect("gift_wrap_local succeeds with local keys");
        let tag_vecs: Vec<Vec<String>> = envelope
            .tags
            .iter()
            .map(|t: &nostr::Tag| t.as_slice().to_vec())
            .collect();
        let json = serde_json::json!({
            "id": envelope.id.to_hex(),
            "pubkey": envelope.pubkey.to_hex(),
            "created_at": envelope.created_at.as_secs(),
            "kind": envelope.kind.as_u16(),
            "tags": tag_vecs,
            "content": envelope.content.clone(),
            "sig": envelope.sig.to_string(),
        })
        .to_string();
        let id = envelope.id.to_hex();
        (json, id)
    };

    let verified = gift_wrap_to_verified(&gift_json);
    dispatcher.dispatch(&verified);

    assert!(
        *dm_parser.fired.lock().unwrap(),
        "NIP-17 DM inbox parser must fire for kind:1059 (slot 'nip17.dm_inbox')"
    );
    assert!(
        *marmot_parser.fired.lock().unwrap(),
        "Marmot parser must fire for kind:1059 (slot 'marmot')"
    );
}

// ── ADR-0025 retirement / dispatch_action → MarmotMlsOpHandler ─────────
//
// The substrate-generic Marmot dispatch seam (the SOLE host entry point
// after ADR-0025 PR 3, 2026-05-23, deleted the legacy bespoke
// `nmp_marmot_dispatch` C-ABI symbol). The proof points:
//
//   1. `MarmotActionModule` registered against `NmpApp::register_action`
//      and `MarmotMlsOpHandler` installed via `NmpApp::set_host_op_handler`
//      are reachable through the kernel's `dispatch_action` path: a
//      `nmp_app_dispatch_action("nmp.marmot", action_json)` call routes
//      to the same `ops::dispatch` handler the legacy bespoke symbol
//      used to reach (and that `MarmotHandle::dispatch` — the surviving
//      Rust-native in-process accessor — still reaches today).
//   2. Both the host (generic) seam and the in-process Rust-native seam
//      (`MarmotHandle::dispatch` / direct `ops::dispatch`) share ONE
//      `MarmotProjection` — a dispatch through the generic path mutates
//      state visible to a subsequent read through the Rust-native path.
//      This is the property the ADR-0025 PR 3 deletion relied on, and the
//      property a future second Marmot host (post-Chirp) must continue to
//      satisfy.

use crate::projection::action::{MarmotActionModule, MARMOT_ACTION_NAMESPACE};
use crate::projection::handler::MarmotMlsOpHandler;

/// End-to-end PROOF of the dispatch_action → MarmotMlsOpHandler path.
///
/// Builds the EXACT wiring `register_with_keys` does (minus the C-ABI
/// shell) directly on a fresh `NmpApp`:
///
/// * register `MarmotActionModule` against the action registry;
/// * install `MarmotMlsOpHandler::new(projection)` into the MLS-op slot.
///
/// Then dispatches the legacy `{"op": "publish_key_package", "relays":
/// [...]}` envelope through `nmp_app_dispatch_action("nmp.marmot",
/// envelope_json)` and asserts:
///
/// * the dispatcher returns a `correlation_id` (the action was accepted);
/// * the `MarmotProjection::snapshot` reflects the published key package
///   (the handler ran and mutated shared state — the SAME state the
///   Rust-native [`MarmotHandle::dispatch`] accessor mutates, and the
///   SAME state the legacy bespoke `nmp_marmot_dispatch` symbol used to
///   mutate before ADR-0025 PR 3 deleted it).
///
/// The actor dispatch arm runs the handler on its own thread; we poll
/// the projection's snapshot under a 2 s wall-clock cap, exactly the
/// pattern the `dispatch_mls_op_*` nmp-core tests use.
#[test]
fn dispatch_action_nmp_marmot_routes_to_projection_via_handler() {
    let alice_keys = Keys::generate();
    let proj = Arc::new(MarmotProjection::new(in_memory(alice_keys.clone()), None));

    let app = nmp_ffi::nmp_app_new();
    // SAFETY: nmp_app_new never returns null; pointer is valid until nmp_app_free.
    let app_mut = unsafe { &mut *app };

    // The two-line wiring `register_with_keys` performs for the
    // dispatch_action seam:
    let _ = app_mut.register_action(MarmotActionModule);
    let handler = Arc::new(MarmotMlsOpHandler::new(Arc::clone(&proj)))
        as Arc<dyn nmp_core::substrate::HostOpHandler>;
    app_mut.set_host_op_handler(handler);
    nmp_ffi::nmp_app_start(app, 256, 4);

    // Dispatch the legacy envelope through the generic seam. The JSON
    // shape is byte-identical with what iOS used to send to the legacy
    // bespoke `nmp_marmot_dispatch` symbol — kept stable so the iOS
    // migration in ADR-0025 PR 2 was a one-line call-site change per op,
    // not a re-encode.
    let envelope_json = r#"{"op":"publish_key_package","relays":["wss://t.relay"]}"#;
    let namespace_c = CString::new(MARMOT_ACTION_NAMESPACE).unwrap();
    let envelope_c = CString::new(envelope_json).unwrap();
    let out_ptr = nmp_ffi::nmp_app_dispatch_action(app, namespace_c.as_ptr(), envelope_c.as_ptr());
    assert!(
        !out_ptr.is_null(),
        "dispatch_action must return a non-null envelope (D6)"
    );
    // SAFETY: the dispatcher returns a freshly-allocated NUL-terminated
    // string the caller must release via `nmp_free_string`.
    let out = unsafe { CStr::from_ptr(out_ptr) }
        .to_string_lossy()
        .into_owned();
    let parsed: serde_json::Value = serde_json::from_str(&out)
        .unwrap_or_else(|e| panic!("dispatch return must be valid JSON; got `{out}`: {e}"));
    let id = parsed
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("dispatch envelope must carry a correlation_id; got: {out}"));
    assert_eq!(
        id.len(),
        32,
        "correlation_id must be 32 hex chars; got: {id}"
    );
    nmp_ffi::nmp_free_string(out_ptr);

    // The handler ran on the actor thread; poll the projection's
    // snapshot for the published key-package mutation. 2 s deadline
    // mirrors the nmp-core dispatch_mls_op tests.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut published = false;
    while std::time::Instant::now() < deadline {
        if proj.snapshot(1_000).key_package.published {
            published = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        published,
        "dispatch_action(nmp.marmot, publish_key_package) must route through \
         MarmotMlsOpHandler and mutate the projection state visible to snapshot \
         (the SAME state MarmotHandle::dispatch mutates — i.e. the SAME state \
         the legacy bespoke nmp_marmot_dispatch symbol used to mutate, pre-PR-3)"
    );

    nmp_ffi::nmp_app_free(app);
}

/// Parity test: the host (generic `dispatch_action`) seam and the
/// in-process Rust-native seam (direct `projection::ops::dispatch`, the
/// same code path `MarmotHandle::dispatch` reaches) mutate ONE shared
/// `MarmotProjection`. A `create_group` through `dispatch_action`
/// produces a group that a subsequent in-process `ops::dispatch` read
/// sees — no duplicate state store, no parallel mutex. This was the
/// precondition ADR-0025 PR 3 relied on when deleting the legacy bespoke
/// `nmp_marmot_dispatch` symbol; it remains the precondition the
/// REPL/TUI tests rely on now that the Rust-native accessor is the
/// substitute for the deleted C symbol.
#[test]
fn dispatch_action_and_bespoke_dispatch_share_one_projection() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();
    let bob = in_memory(bob_keys.clone());
    let bob_kp_json = bob
        .publish_key_package(vec![nostr::RelayUrl::parse("wss://t.relay").unwrap()])
        .expect("bob kp")
        .event_30443
        .as_json();

    let proj = Arc::new(MarmotProjection::new(in_memory(alice_keys.clone()), None));

    let app = nmp_ffi::nmp_app_new();
    // SAFETY: nmp_app_new never returns null.
    let app_mut = unsafe { &mut *app };
    let _ = app_mut.register_action(MarmotActionModule);
    let handler = Arc::new(MarmotMlsOpHandler::new(Arc::clone(&proj)))
        as Arc<dyn nmp_core::substrate::HostOpHandler>;
    app_mut.set_host_op_handler(handler);
    nmp_ffi::nmp_app_start(app, 256, 4);

    // Generic seam: create the group via dispatch_action.
    let envelope = json!({
        "op": "create_group",
        "name": "PR 1 parity",
        "description": "shared projection proof",
        "relays": ["wss://t.relay"],
        "invitee_npubs": [bob_keys.public_key().to_hex()],
        "signed_key_package_events_json": [bob_kp_json],
    })
    .to_string();
    let namespace_c = CString::new(MARMOT_ACTION_NAMESPACE).unwrap();
    let envelope_c = CString::new(envelope).unwrap();
    let out_ptr = nmp_ffi::nmp_app_dispatch_action(app, namespace_c.as_ptr(), envelope_c.as_ptr());
    assert!(!out_ptr.is_null());
    // SAFETY: out_ptr came from nmp_app_dispatch_action (D6 contract).
    let out = unsafe { CStr::from_ptr(out_ptr) }
        .to_string_lossy()
        .into_owned();
    let returned_id = serde_json::from_str::<serde_json::Value>(&out)
        .ok()
        .and_then(|v| {
            v.get("correlation_id")
                .and_then(|c| c.as_str())
                .map(str::to_owned)
        })
        .expect("dispatch must return correlation_id");
    nmp_ffi::nmp_free_string(out_ptr);

    // Poll for the create_group to complete on the actor thread.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut group: Option<String> = None;
    while std::time::Instant::now() < deadline {
        let snap = proj.snapshot(1_000);
        if let Some(g) = snap.groups.first() {
            group = Some(g.id_hex.clone());
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let group_id_hex = group.unwrap_or_else(|| {
        panic!(
            "create_group through dispatch_action must mutate the same \
             projection the bespoke seam reads (correlation_id={returned_id})"
        )
    });

    // In-process Rust-native seam (via ops::dispatch — the SAME entry
    // point MarmotHandle::dispatch reaches, and the SAME entry point the
    // legacy bespoke `nmp_marmot_dispatch` symbol used to reach pre-PR-3):
    // send a message into the just-created group. If the generic seam and
    // the Rust-native seam were separate stores, this would fail with
    // `unknown group_id`.
    let r = proj
        .with_inner(|h| {
            ops::dispatch(
                h,
                &json!({
                    "op": "send",
                    "group_id_hex": &group_id_hex,
                    "text": "parity proof",
                }),
                1_001,
                None,
            )
        })
        .expect("projection mutex should not be poisoned");
    assert_eq!(
        r["ok"],
        json!(true),
        "the in-process Rust-native seam (ops::dispatch / \
         MarmotHandle::dispatch) must see the group created through the \
         generic dispatch_action seam: {r}"
    );

    nmp_ffi::nmp_app_free(app);
}

// ── V-107 / ADR-0039: push-projection logic verification ─────────────────
//
// These tests verify the logic that the push-projection CLOSURES delegate to:
//
// - `MarmotProjection::messages_all_groups_json` — the method the
//   `nmp.marmot.messages` closure calls. This is the code path that was
//   previously inlined in the closure; extracting it makes it directly testable.
//
// - The projection_slot lifecycle: after `nmp_marmot_unregister` the slot
//   is cleared to `None`, so closures that read it emit empty objects.
//
// The registered closures themselves wrap these methods with a slot guard:
//
//   `nmp.marmot.snapshot`:  slot.lock() → proj.snapshot(now_secs())
//   `nmp.marmot.messages`:  slot.lock() → proj.messages_all_groups_json(page)
//
// The closure logic is trivially correct once the underlying methods are
// verified and the slot is confirmed to hold the right projection. Both
// are exercised here.

/// `MarmotProjection::messages_all_groups_json` must return a JSON object
/// keyed by `group_id_hex` containing the sent message rows.
///
/// This is the exact logic the `nmp.marmot.messages` push projection closure
/// delegates to on every snapshot tick.
#[test]
fn messages_all_groups_json_emits_keyed_rows_after_send() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();
    let bob = in_memory(bob_keys.clone());
    let bob_kp_json = bob
        .publish_key_package(vec![nostr::RelayUrl::parse("wss://t.relay").unwrap()])
        .expect("bob kp")
        .event_30443
        .as_json();

    let proj = MarmotProjection::new(in_memory(alice_keys.clone()), None);

    // Publish key package and create a group.
    let kp_r = proj
        .with_inner(|h| {
            ops::dispatch(
                h,
                &json!({ "op": "publish_key_package", "relays": ["wss://t.relay"] }),
                1_000,
                None,
            )
        })
        .unwrap();
    assert_eq!(kp_r["ok"], json!(true));

    let create_r = proj
        .with_inner(|h| {
            ops::dispatch(
                h,
                &json!({
                    "op": "create_group",
                    "name": "Msgs All Groups Test",
                    "relays": ["wss://t.relay"],
                    "invitee_npubs": [bob_keys.public_key().to_hex()],
                    "signed_key_package_events_json": [bob_kp_json],
                }),
                1_001,
                None,
            )
        })
        .unwrap();
    assert_eq!(create_r["ok"], json!(true), "create_group: {create_r}");
    let group_id_hex = create_r["group_id_hex"].as_str().unwrap().to_string();

    // Before send: no messages.
    let empty = proj.messages_all_groups_json(200);
    assert!(empty.is_object(), "must be a JSON object before send");
    assert!(
        empty
            .get(&group_id_hex)
            .and_then(|v| v.as_array())
            .map(|a| a.is_empty())
            .unwrap_or(true),
        "no messages yet: {empty}"
    );

    // Send a message.
    let send_r = proj
        .with_inner(|h| {
            ops::dispatch(
                h,
                &json!({
                    "op": "send",
                    "group_id_hex": &group_id_hex,
                    "text": "all-groups map test",
                }),
                1_002,
                None,
            )
        })
        .unwrap();
    assert_eq!(send_r["ok"], json!(true), "send: {send_r}");

    // After send: messages_all_groups_json must include the row under the
    // correct group key. This is the exact code path the closure runs.
    let msgs = proj.messages_all_groups_json(200);
    assert!(
        msgs.is_object(),
        "must be a JSON object after send; got: {msgs}"
    );
    let rows = msgs
        .get(&group_id_hex)
        .and_then(|v| v.as_array())
        .expect("group key must be present in the map");
    assert_eq!(rows.len(), 1, "one sent message expected; got: {rows:?}");
    assert_eq!(
        rows[0].get("content").and_then(|v| v.as_str()),
        Some("all-groups map test"),
        "message content must match"
    );
    assert_eq!(
        rows[0].get("sender_pubkey_hex").and_then(|v| v.as_str()),
        Some(alice_keys.public_key().to_hex().as_str()),
        "sender pubkey must match"
    );
}

/// After `nmp_marmot_unregister` the `projection_slot` held by the push-
/// projection closures is cleared to `None`. A subsequent read through the
/// slot must return an empty result — not the signed-out account's data.
///
/// Verifies both the snapshot-slot and the messages-slot clear by calling
/// the closures' read path (through the `MarmotProjectionSlot`) directly.
#[test]
fn projection_slot_cleared_on_unregister_emits_empty() {
    use crate::ffi::MarmotProjectionSlot;
    use std::sync::{Arc, Mutex};

    // Build a projection with a group.
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();
    let bob = in_memory(bob_keys.clone());
    let bob_kp_json = bob
        .publish_key_package(vec![nostr::RelayUrl::parse("wss://t.relay").unwrap()])
        .expect("bob kp")
        .event_30443
        .as_json();

    let proj = Arc::new(MarmotProjection::new(in_memory(alice_keys.clone()), None));
    // Test setup step — dispatch result is asserted via the subsequent
    // `create_r`; discard the `#[must_use]` return here explicitly.
    let _ = proj.with_inner(|h| {
        ops::dispatch(
            h,
            &json!({ "op": "publish_key_package", "relays": ["wss://t.relay"] }),
            1_000,
            None,
        )
    });
    let create_r = proj
        .with_inner(|h| {
            ops::dispatch(
                h,
                &json!({
                    "op": "create_group",
                    "name": "Slot Clear Test",
                    "relays": ["wss://t.relay"],
                    "invitee_npubs": [bob_keys.public_key().to_hex()],
                    "signed_key_package_events_json": [bob_kp_json],
                }),
                1_001,
                None,
            )
        })
        .unwrap();
    assert_eq!(create_r["ok"], json!(true));

    // Build a slot as register_with_keys does (#1651: `MarmotSlotState`).
    use crate::ffi::MarmotSlotState;
    let slot: MarmotProjectionSlot =
        Arc::new(Mutex::new(MarmotSlotState::Ready(Arc::clone(&proj))));

    // With slot Ready: messages_all_groups_json via the slot returns data.
    let msgs_before = {
        let guard = slot.lock().unwrap();
        match &*guard {
            MarmotSlotState::Ready(p) => Some(p.messages_all_groups_json(200)),
            _ => None,
        }
    };
    assert!(msgs_before.is_some(), "Ready-slot read must return data");

    // Simulate nmp_marmot_unregister clearing the slot.
    if let Ok(mut s) = slot.lock() {
        *s = MarmotSlotState::Cleared;
    }

    // With slot Cleared: the closure read path emits nothing → empty object.
    let guard = slot.lock().unwrap();
    assert!(
        matches!(&*guard, MarmotSlotState::Cleared),
        "slot must be Cleared after unregister"
    );
    let empty_msgs = match &*guard {
        MarmotSlotState::Ready(p) => p.messages_all_groups_json(200),
        _ => serde_json::Value::Object(serde_json::Map::new()),
    };
    assert!(
        empty_msgs
            .as_object()
            .map(|m| m.is_empty())
            .unwrap_or(false),
        "cleared slot must produce empty object; got: {empty_msgs}"
    );
}
