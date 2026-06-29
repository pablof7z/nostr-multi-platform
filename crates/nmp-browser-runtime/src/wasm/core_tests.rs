//! Unit tests for `NmpRuntimeCore` (split out of `wasm/core.rs` to keep it
//! under the 500-LOC ceiling, AGENTS.md). Declared from `core.rs` via
//! `#[cfg(test)] #[path = "core_tests.rs"] mod tests;` so it shares the parent
//! module's private surface through `use super::*`.

use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

use super::*;
use nmp_signers::{LocalKeySigner, Signer};

const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

fn start_req() -> String {
    serde_json::json!({
        "type": "start",
        "app_id": "chirp",
        "relays": [],
        "relay_bootstrap": [],
        "database_name": "chirp-test",
        "correlation_id": "start-1"
    })
    .to_string()
}

fn set_identity_req() -> String {
    serde_json::json!({
        "type": "set_identity",
        "kind": "nip07",
        "pubkey_hex": PK,
        "correlation_id": "id-1"
    })
    .to_string()
}

fn resolve_profile_ref_req(correlation_id: &str) -> String {
    serde_json::json!({
        "type": "resolve_ref",
        "namespace": 0,
        "key": PK,
        "consumer_id": "profile-card:stable",
        "shape": 1,
        "liveness": 0,
        "correlation_id": correlation_id
    })
    .to_string()
}

fn release_profile_ref_req(correlation_id: &str) -> String {
    serde_json::json!({
        "type": "release_ref",
        "namespace": 0,
        "key": PK,
        "consumer_id": "profile-card:stable",
        "correlation_id": correlation_id
    })
    .to_string()
}

#[test]
fn new_core_has_no_handle() {
    let core = NmpRuntimeCore::new();
    assert!(core.handle.is_none());
}

#[test]
fn new_core_has_no_injected_store() {
    let core = NmpRuntimeCore::new();
    assert!(
        core.injected_store.is_none(),
        "fresh core must default to no durable store (→ in_memory at start)"
    );
}

#[test]
fn new_core_has_no_store_open_failure() {
    let core = NmpRuntimeCore::new();
    assert!(
        core.store_open_failure.is_none(),
        "fresh core must default to no degraded-open reason (healthy until proven otherwise)"
    );
}

// ── #1007 PR-8: degraded OPFS open surfaces store_open_failure ─────────────

/// A degraded-open reason parked on the core (as `prepare_store` does on an
/// OPFS open failure) must reach the started handle's kernel and surface
/// through the Tier-3 `store_open_failure` channel. This is the native,
/// always-runnable proof of the wasm path: core → handle_start →
/// BrowserAppBuilder::with_store_open_failure → kernel. The kernel→snapshot
/// leg is proven by `nmp-core`'s `v67_store_open_failure_tests`.
#[test]
fn degraded_open_reason_surfaces_on_started_handle() {
    let mut core = NmpRuntimeCore::new();
    let reason = crate::wasm::store_failure::SECOND_TAB_POOL_LOCK.to_string();
    core.set_store_open_failure(reason.clone());

    let resp = core.handle_json_request(&start_req());
    assert!(resp.contains("running"), "resp={resp}");

    let handle = core.handle.as_ref().expect("handle after start");
    assert_eq!(
        handle.store_open_failure().as_deref(),
        Some(reason.as_str()),
        "a degraded OPFS open must surface the SAME store_open_failure native LMDB does"
    );
}

/// Control: a healthy start (no parked reason) must leave `store_open_failure`
/// absent — guards the assertion above against a false positive.
#[test]
fn healthy_start_has_no_store_open_failure() {
    let mut core = NmpRuntimeCore::new();
    let _ = core.handle_json_request(&start_req());
    let handle = core.handle.as_ref().expect("handle after start");
    assert!(
        handle.store_open_failure().is_none(),
        "a healthy in-memory start must NOT report a store_open_failure"
    );
}

// ── #1007 PR-7: per-app OPFS database-name composition ────────────────────

#[test]
fn opfs_database_name_namespaces_by_app_id() {
    assert_eq!(super::opfs_database_name("chirp", "feed"), "chirp-feed");
    assert_eq!(
        super::opfs_database_name("  chirp ", " feed "),
        "chirp-feed"
    );
    assert_eq!(super::opfs_database_name("chirp", ""), "chirp");
    assert_eq!(super::opfs_database_name("", "feed"), "feed");
    assert_eq!(super::opfs_database_name("", ""), "nmp");
}

#[test]
fn hello_accepted_on_correct_version() {
    let mut core = NmpRuntimeCore::new();
    let req = serde_json::json!({
        "type": "hello",
        "app_id": "test",
        "platform": "web",
        "protocol_version": 1
    });
    let resp = core.handle_json_request(&req.to_string());
    assert!(resp.contains("hello_accepted"), "resp={resp}");
}

#[test]
fn hello_rejected_on_wrong_version() {
    let mut core = NmpRuntimeCore::new();
    let req = serde_json::json!({
        "type": "hello",
        "app_id": "test",
        "platform": "web",
        "protocol_version": 99
    });
    let resp = core.handle_json_request(&req.to_string());
    assert!(resp.contains("protocol_mismatch"), "resp={resp}");
}

#[test]
fn start_creates_handle() {
    let mut core = NmpRuntimeCore::new();
    let resp = core.handle_json_request(&start_req());
    assert!(resp.contains("running"), "resp={resp}");
    assert!(
        core.handle.is_some(),
        "handle should be populated after start"
    );
}

#[test]
fn request_before_start_returns_not_started() {
    let mut core = NmpRuntimeCore::new();
    let resp = core.handle_json_request(&set_identity_req());
    assert!(resp.contains("not_started"), "resp={resp}");
}

#[test]
fn recent_routing_decisions_returns_error_before_start() {
    let core = NmpRuntimeCore::new();
    let s = core.recent_routing_decisions();
    assert!(s.contains("not_started"), "s={s}");
}

#[test]
fn recent_routing_decisions_returns_string_after_start() {
    let mut core = NmpRuntimeCore::new();
    let _ = core.handle_json_request(&start_req());
    let s = core.recent_routing_decisions();
    assert!(!s.contains("not_started"), "s={s}");
}

// ── BLOCKER 1: wake ordering ──────────────────────────────────────────────

/// Snapshot sink installed BEFORE start must fire after start. This is the
/// native-CI proxy for the wasm wake-ordering fix (#2139 BLOCKER 1): if
/// the sink works before/after start, the wake (wasm-only) is also
/// correctly deferred.
#[test]
fn snapshot_sink_set_before_start_receives_bytes_after_start() {
    let mut core = NmpRuntimeCore::new();

    let received = Arc::new(AtomicBool::new(false));
    let received2 = Arc::clone(&received);
    core.set_snapshot_sink(Some(Box::new(move |_bytes| {
        received2.store(true, Ordering::SeqCst);
    })));

    // Sink is set; start is NOT done yet.
    assert!(core.handle.is_none());

    // Now start.
    let _ = core.handle_json_request(&start_req());
    assert!(core.handle.is_some());

    // Push snapshot — sink must be called despite being set before start.
    core.push_snapshot_bytes_if_sink();
    assert!(
        received.load(Ordering::SeqCst),
        "sink set before start must fire after start (#2139 BLOCKER 1)"
    );
}

#[test]
fn warm_profile_resolve_acknowledges_without_snapshot_push() {
    let mut core = NmpRuntimeCore::new();

    let pushes = Arc::new(AtomicUsize::new(0));
    let pushes2 = Arc::clone(&pushes);
    core.set_snapshot_sink(Some(Box::new(move |_bytes| {
        pushes2.fetch_add(1, Ordering::SeqCst);
    })));

    let _ = core.handle_json_request(&start_req());
    core.push_snapshot_bytes_if_sink();
    let baseline = pushes.load(Ordering::SeqCst);
    assert!(baseline > 0, "start must produce the baseline snapshot");

    let first = core.handle_json_request(&resolve_profile_ref_req("resolve-1"));
    assert!(first.contains("action_accepted"), "first={first}");
    core.push_snapshot_bytes_if_sink();
    let after_first = pushes.load(Ordering::SeqCst);
    assert_eq!(
        after_first,
        baseline + 1,
        "first profile resolve mutates the kernel and pushes one snapshot"
    );

    let second = core.handle_json_request(&resolve_profile_ref_req("resolve-2"));
    assert!(second.contains("action_accepted"), "second={second}");
    core.push_snapshot_bytes_if_sink();
    assert_eq!(
        pushes.load(Ordering::SeqCst),
        after_first,
        "warm identical profile resolve must not push a redundant snapshot"
    );
}

#[test]
fn noop_profile_release_acknowledges_without_snapshot_push() {
    let mut core = NmpRuntimeCore::new();

    let pushes = Arc::new(AtomicUsize::new(0));
    let pushes2 = Arc::clone(&pushes);
    core.set_snapshot_sink(Some(Box::new(move |_bytes| {
        pushes2.fetch_add(1, Ordering::SeqCst);
    })));

    let _ = core.handle_json_request(&start_req());
    core.push_snapshot_bytes_if_sink();
    let _ = core.handle_json_request(&resolve_profile_ref_req("resolve-1"));
    core.push_snapshot_bytes_if_sink();
    let after_resolve = pushes.load(Ordering::SeqCst);

    let first = core.handle_json_request(&release_profile_ref_req("release-1"));
    assert!(first.contains("action_accepted"), "first={first}");
    core.push_snapshot_bytes_if_sink();
    let after_first_release = pushes.load(Ordering::SeqCst);
    assert_eq!(
        after_first_release,
        after_resolve + 1,
        "first profile release mutates the kernel and pushes one snapshot"
    );

    let second = core.handle_json_request(&release_profile_ref_req("release-2"));
    assert!(second.contains("action_accepted"), "second={second}");
    core.push_snapshot_bytes_if_sink();
    assert_eq!(
        pushes.load(Ordering::SeqCst),
        after_first_release,
        "second release of the same profile consumer must not push a redundant snapshot"
    );
}

// ── BLOCKER 2: sign terminals emitted from deliver_signer_response ────────

/// A `deliver_signer_response` with error must return `sign_failed` from
/// `handle_json` (not an empty array). Proves sign terminals travel through
/// the sync pump path (#2139 BLOCKER 2).
#[test]
fn deliver_signer_response_failure_emits_sign_failed() {
    let mut core = NmpRuntimeCore::new();
    let _ = core.handle_json_request(&start_req());
    let _ = core.handle_json_request(&set_identity_req());

    // Begin a sign round-trip so the kernel parks one.
    let sign_resp = core.handle_json_request(
        &serde_json::json!({
            "type": "begin_sign",
            "account_pubkey": PK,
            "unsigned_json": r#"{"kind":1,"created_at":0,"tags":[],"content":"hi"}"#
        })
        .to_string(),
    );
    let events: serde_json::Value = serde_json::from_str(&sign_resp).expect("valid JSON");
    let cid = events[0]["correlation_id"]
        .as_str()
        .expect("sign_request must have correlation_id")
        .to_string();

    // Deliver a failure — must produce sign_failed, not empty array.
    let resp = core.handle_json_request(
        &serde_json::json!({
            "type": "deliver_signer_response",
            "correlation_id": cid,
            "error": "user rejected"
        })
        .to_string(),
    );
    assert!(
        resp.contains("sign_failed"),
        "deliver_signer_response with error must emit sign_failed, got: {resp}"
    );
    assert!(
        resp.contains(&cid),
        "sign_failed must echo back correlation_id, got: {resp}"
    );
}

// ── BLOCKER 3: nmp_encode_npub JSON shape ─────────────────────────────────

/// `nmp_encode_npub` must return a JSON object with `npub` and `npubShort`
/// fields so `wasmBridge.ts`'s `JSON.parse(json)` call works (#2139 BLOCKER 3).
#[test]
fn encode_npub_returns_json_with_npub_and_npub_short() {
    let json = crate::wasm::nmp_encode_npub(PK);
    assert!(
        !json.is_empty(),
        "must return non-empty string for valid hex"
    );
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("must be valid JSON");
    let npub = parsed["npub"]
        .as_str()
        .expect("npub field must be a string");
    let npub_short = parsed["npubShort"]
        .as_str()
        .expect("npubShort field must be a string");
    assert!(
        npub.starts_with("npub1"),
        "npub must start with npub1, got: {npub}"
    );
    assert!(!npub_short.is_empty(), "npubShort must be non-empty");
    assert!(
        npub_short.contains('…'),
        "npubShort must be abbreviated with ellipsis"
    );
}

// ── HIGH 4: identity_relays applied at set_active_account ────────────────

/// Sending `set_identity` with `identity_relays` must result in those relays
/// being added to the configured relay list (#2139 HIGH 4).
#[test]
fn set_identity_with_identity_relays_configures_relays() {
    let mut core = NmpRuntimeCore::new();
    let _ = core.handle_json_request(&start_req());

    // Start with zero configured relays.
    let relay_count_before = core
        .handle
        .as_ref()
        .unwrap()
        .configured_relays()
        .as_slice()
        .len();
    assert_eq!(relay_count_before, 0, "no relays before set_identity");

    // set_identity with identity_relays.
    let resp = core.handle_json_request(
        &serde_json::json!({
            "type": "set_identity",
            "kind": "nip07",
            "pubkey_hex": PK,
            "correlation_id": "id-1",
            "identity_relays": [
                { "url": "wss://relay.example.com", "read": true, "write": true }
            ]
        })
        .to_string(),
    );
    assert!(resp.contains("action_accepted"), "resp={resp}");

    let relay_count_after = core
        .handle
        .as_ref()
        .unwrap()
        .configured_relays()
        .as_slice()
        .len();
    assert!(
        relay_count_after > relay_count_before,
        "identity relay must be added to configured relays (#2139 HIGH 4)"
    );
}

#[test]
fn local_key_identity_derives_account_and_redacts_debug() {
    let mut core = NmpRuntimeCore::new();
    let _ = core.handle_json_request(&start_req());

    let req = serde_json::json!({
        "type": "set_identity",
        "kind": "local_key",
        "pubkey_hex": "",
        "secret_key_bech32": TEST_NSEC,
        "correlation_id": "id-local"
    });
    let parsed: super::WorkerRequest =
        serde_json::from_str(&req.to_string()).expect("set_identity parses");
    let debug = format!("{parsed:?}");
    assert!(
        !debug.contains(TEST_NSEC),
        "debug formatting must never expose the nsec: {debug}"
    );
    assert!(
        debug.contains("[redacted]"),
        "debug formatting must mark the redacted secret field: {debug}"
    );

    let resp = core.handle_json_request(&req.to_string());
    assert!(resp.contains("action_accepted"), "resp={resp}");

    let signer = LocalKeySigner::from_nsec(TEST_NSEC).expect("valid test nsec");
    let expected_pubkey = signer.pubkey().to_hex();
    let active = core
        .handle
        .as_ref()
        .and_then(|handle| handle.active_account_pubkey_inner())
        .expect("active account after local-key sign in");
    assert_eq!(active, expected_pubkey);
}

#[test]
fn nip46_identity_accepts_bunker_uri_without_pubkey_and_redacts_debug() {
    let mut core = NmpRuntimeCore::new();
    let _ = core.handle_json_request(&start_req());
    let remote = nostr::Keys::generate();
    let bunker_uri = format!(
        "bunker://{}?relay=wss://relay.example.com&secret=super-secret",
        remote.public_key().to_hex()
    );

    let req = serde_json::json!({
        "type": "set_identity",
        "kind": "nip46",
        "bunker_uri": bunker_uri,
        "correlation_id": "id-nip46"
    });
    let parsed: super::WorkerRequest =
        serde_json::from_str(&req.to_string()).expect("set_identity parses");
    let debug = format!("{parsed:?}");
    assert!(
        !debug.contains("super-secret"),
        "debug formatting must never expose bunker secrets: {debug}"
    );
    assert!(
        debug.contains("[redacted]"),
        "debug formatting must mark the redacted bunker URI: {debug}"
    );

    let resp = core.handle_json_request(&req.to_string());
    assert!(resp.contains("action_accepted"), "resp={resp}");
    assert!(
        core.handle
            .as_ref()
            .and_then(|handle| handle.active_account_pubkey_inner())
            .is_none(),
        "NIP-46 account is selected after SignerReady, not from host pubkey"
    );
}

#[test]
fn nip46_identity_requires_bunker_uri() {
    let mut core = NmpRuntimeCore::new();
    let _ = core.handle_json_request(&start_req());
    let resp = core.handle_json_request(
        &serde_json::json!({
            "type": "set_identity",
            "kind": "nip46",
            "correlation_id": "id-nip46-missing"
        })
        .to_string(),
    );

    assert!(resp.contains("capability_failure"), "resp={resp}");
    assert!(
        resp.contains("missing_nip46_bunker_uri"),
        "missing bunker URI must surface stable error: {resp}"
    );
}
