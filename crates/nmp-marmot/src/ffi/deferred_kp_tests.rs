//! Deferred KP-completion tests (PR-1 — marmot-create-fix ladder).
//!
//! Proves KP-gated ops park under their correlation id, retry on KP arrival,
//! emit exactly one terminal verdict, and expire on a later ingest/snapshot edge.

use crate::projection::ops::{self, ingest_signed_event_core};
use crate::projection::pending::PENDING_OP_EXPIRY_SECS;
use crate::projection::state::MarmotProjection;
use crate::service::MarmotService;
use mdk_sqlite_storage::MdkSqliteStorage;
use nostr::Keys;
use serde_json::json;

fn in_memory(keys: Keys) -> MarmotService {
    let storage = MdkSqliteStorage::new_in_memory().expect("in-memory mls storage");
    MarmotService::from_storage(storage, keys, Default::default())
}

/// Create a KP event for `keys` so it can be used in `ingest_signed_event_core`.
fn make_kp_event(keys: &Keys) -> nostr::Event {
    let relay = nostr::RelayUrl::parse("wss://t.relay").unwrap();
    let service = in_memory(keys.clone());
    service
        .publish_key_package(vec![relay])
        .expect("kp")
        .event_30443
        .clone()
}

/// A `create_group` with a missing KP and a `correlation_id` returns
/// `{"pending":true}` — NOT a terminal `{"ok":false}`.
///
/// After the missing peer's KP event is fed via `ingest_signed_event_core`,
/// the group IS created (visible in the snapshot). The actor channel is null
/// in this test (no real `NmpApp`), so `push_actor_command` silently no-ops
/// and we verify group creation via snapshot state instead of
/// `RecordActionSuccess`.
#[test]
fn create_group_with_missing_kp_parks_and_retries_on_kp_arrival() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let proj = MarmotProjection::new(in_memory(alice_keys.clone()), None);

    // Alice publishes her own KP (needed by the group op).
    proj.with_inner(|h| {
        ops::dispatch_json_for_tests(
            h,
            json!({ "op": "publish_key_package", "relays": ["wss://t.relay"] }),
            1_000,
            None,
        )
    })
    .unwrap();

    // Dispatch create_group WITH a correlation_id but WITHOUT Bob's KP.
    let r = proj
        .with_inner(|h| {
            ops::dispatch_json_for_tests(
                h,
                json!({
                    "op": "create_group",
                    "name": "Deferred Test",
                    "relays": ["wss://t.relay"],
                    "invitee_npubs": [bob_keys.public_key().to_hex()],
                }),
                1_001,
                Some("corr-deferred-1"),
            )
        })
        .unwrap();

    // Must be a pending envelope, NOT a terminal failure.
    assert_eq!(
        r.get("pending").and_then(|v| v.as_bool()),
        Some(true),
        "missing-KP create_group with correlation_id must return pending: {r}"
    );
    assert_eq!(
        r.get("ok").and_then(|v| v.as_bool()),
        None,
        "pending envelope must not carry an ok field: {r}"
    );
    assert_eq!(
        r.get("correlation_id").and_then(|v| v.as_str()),
        Some("corr-deferred-1"),
        "pending envelope must echo the original correlation_id: {r}"
    );

    // Snapshot: no groups yet (the op is parked).
    let snap = proj.snapshot(1_001);
    assert!(
        snap.groups.is_empty(),
        "group must not appear while op is pending: {snap:?}"
    );

    // Now ingest Bob's KP — this should trigger the retry.
    let bob_kp = make_kp_event(&bob_keys);
    assert_eq!(bob_kp.kind.as_u16(), 30443, "KP event must be kind 30443");
    assert_eq!(
        bob_kp.pubkey.to_hex(),
        bob_keys.public_key().to_hex(),
        "KP event must be authored by Bob"
    );
    // Use now_secs consistent with the parking time (1_001) so the expiry
    // gate (now - created_at >= 60s) does not fire prematurely.
    let kp_arrival_secs: u64 = 1_002;
    let ingest_result = proj.with_inner(|h| ingest_signed_event_core(h, &bob_kp, kp_arrival_secs));
    // Verify ingest did not error.
    let _ingest_ok = ingest_result
        .expect("with_inner lock")
        .expect("ingest_signed_event_core should not Err");

    // Snapshot: group now appears (the retry executed successfully).
    let snap = proj.snapshot(1_002);
    assert_eq!(
        snap.groups.len(),
        1,
        "group must appear in snapshot after KP arrival triggers retry: {snap:?}"
    );
    assert_eq!(snap.groups[0].name, "Deferred Test");
    assert_eq!(
        snap.groups[0].members.len(),
        2,
        "both Alice and Bob must be members"
    );

    // Pending store must be empty after successful retry.
    let summaries = proj.with_inner(|h| h.pending_op_summaries()).unwrap();
    assert!(
        summaries.is_empty(),
        "pending store must be cleared after successful retry: {summaries:?}"
    );
}

/// A `create_group` with a missing KP parks the op, but if the KP does NOT
/// arrive within `PENDING_OP_EXPIRY_SECS`, the next KP ingest edge evicts the
/// op with a terminal failure.
///
/// We simulate time passing by using a `now_secs` value in the future when
/// the unrelated KP arrives. The actor channel is null, so the
/// `RecordActionFailure` command is silently dropped — we verify via the
/// pending store being empty and the snapshot having no groups.
#[test]
fn pending_op_expires_after_deadline_on_next_ingest_edge() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();
    let carol_keys = Keys::generate(); // Carol's KP arrives (unrelated to the blocking op).

    let proj = MarmotProjection::new(in_memory(alice_keys.clone()), None);

    // Alice publishes her own KP.
    proj.with_inner(|h| {
        ops::dispatch_json_for_tests(
            h,
            json!({ "op": "publish_key_package", "relays": ["wss://t.relay"] }),
            1_000,
            None,
        )
    })
    .unwrap();

    // Park the op waiting for Bob's KP (which will never arrive before expiry).
    let r = proj
        .with_inner(|h| {
            ops::dispatch_json_for_tests(
                h,
                json!({
                    "op": "create_group",
                    "name": "Expiry Test",
                    "relays": ["wss://t.relay"],
                    "invitee_npubs": [bob_keys.public_key().to_hex()],
                }),
                1_001,
                Some("corr-expiry-1"),
            )
        })
        .unwrap();
    assert_eq!(
        r.get("pending").and_then(|v| v.as_bool()),
        Some(true),
        "must be pending: {r}"
    );

    // Ingest Carol's KP (unrelated to the blocked op) at a time PAST the
    // expiry threshold.
    let expired_now = 1_001 + PENDING_OP_EXPIRY_SECS + 1;
    let _carol_kp = make_kp_event(&carol_keys);
    // Drive the cache edge directly so the synthetic `expired_now` trips expiry.
    let ready = proj
        .with_inner(|h| h.handle_key_package_cached(&carol_keys.public_key().to_hex(), expired_now))
        .unwrap();

    // handle_key_package_cached returns ready ops — Carol's KP doesn't match
    // the blocked op (which blocks on Bob). So ready is empty.
    // BUT the expiry eviction fires: the pending op was created at 1_001 and
    // `expired_now > 1_001 + EXPIRY`, so it is evicted.
    assert!(
        ready.is_empty(),
        "Carol's KP does not unblock the Bob-gated op: {ready:?}"
    );

    // Pending store must be empty after expiry eviction.
    let summaries = proj.with_inner(|h| h.pending_op_summaries()).unwrap();
    assert!(
        summaries.is_empty(),
        "expired op must be evicted from the pending store: {summaries:?}"
    );

    // Snapshot: no groups (the op expired without completing).
    let snap = proj.snapshot(expired_now);
    assert!(
        snap.groups.is_empty(),
        "no group must appear after expiry: {snap:?}"
    );
}

/// A duplicate create_group (same op + same missing pubkey fingerprint) is
/// rejected while the first is pending. The response is still `{"pending":true}`
/// but includes `"duplicate":true` and references the first correlation_id.
#[test]
fn duplicate_create_group_while_pending_is_rejected_as_duplicate() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let proj = MarmotProjection::new(in_memory(alice_keys.clone()), None);

    proj.with_inner(|h| {
        ops::dispatch_json_for_tests(
            h,
            json!({ "op": "publish_key_package", "relays": ["wss://t.relay"] }),
            1_000,
            None,
        )
    })
    .unwrap();

    // First dispatch — parks the op.
    let r1 = proj
        .with_inner(|h| {
            ops::dispatch_json_for_tests(
                h,
                json!({
                    "op": "create_group",
                    "name": "g",
                    "relays": ["wss://t.relay"],
                    "invitee_npubs": [bob_keys.public_key().to_hex()],
                }),
                1_001,
                Some("corr-first"),
            )
        })
        .unwrap();
    assert_eq!(r1["pending"], json!(true));

    // Second identical dispatch — must be rejected as duplicate.
    let r2 = proj
        .with_inner(|h| {
            ops::dispatch_json_for_tests(
                h,
                json!({
                    "op": "create_group",
                    "name": "g",
                    "relays": ["wss://t.relay"],
                    "invitee_npubs": [bob_keys.public_key().to_hex()],
                }),
                1_002,
                Some("corr-second"),
            )
        })
        .unwrap();
    assert_eq!(
        r2["pending"],
        json!(true),
        "duplicate must still be pending: {r2}"
    );
    assert_eq!(
        r2["duplicate"],
        json!(true),
        "duplicate flag must be set: {r2}"
    );
    assert_eq!(
        r2.get("correlation_id").and_then(|v| v.as_str()),
        Some("corr-first"),
        "duplicate must reference the first correlation_id: {r2}"
    );

    // Only one pending op must exist (the first).
    let summaries = proj.with_inner(|h| h.pending_op_summaries()).unwrap();
    assert_eq!(
        summaries.len(),
        1,
        "duplicate create must not add a second pending op: {summaries:?}"
    );
    assert_eq!(summaries[0].0, "corr-first");
}

/// When `create_group` is called WITHOUT a `correlation_id` (REPL / tests /
/// `MarmotHandle::dispatch`) and the KP is missing, the old terminal
/// `{"ok":false,"error":"key_package_unavailable"}` response is returned
/// unchanged — the deferred path only activates when a correlation_id is
/// present.
#[test]
fn create_group_without_correlation_id_returns_terminal_soft_fail() {
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
                None, // no correlation_id → old terminal behavior
            )
        })
        .unwrap();
    assert_eq!(r["ok"], json!(false), "must be terminal failure: {r}");
    assert_eq!(r["error"], json!("key_package_unavailable"));
    assert_eq!(
        r.get("pending").and_then(|v| v.as_bool()),
        None,
        "no pending flag in terminal response: {r}"
    );
}

/// A parked op whose KP never arrives expires on a later snapshot edge.
#[test]
fn pending_op_expires_on_snapshot_edge_without_any_further_ingest() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let proj = MarmotProjection::new(in_memory(alice_keys.clone()), None);
    proj.with_inner(|h| {
        ops::dispatch_json_for_tests(
            h,
            json!({ "op": "publish_key_package", "relays": ["wss://t.relay"] }),
            1_000,
            None,
        )
    })
    .unwrap();

    // Park the op at t=1_001.
    let r = proj
        .with_inner(|h| {
            ops::dispatch_json_for_tests(
                h,
                json!({
                    "op": "create_group",
                    "name": "Snapshot Expiry",
                    "relays": ["wss://t.relay"],
                    "invitee_npubs": [bob_keys.public_key().to_hex()],
                }),
                1_001,
                Some("corr-snap-expiry"),
            )
        })
        .unwrap();
    assert_eq!(r["pending"], json!(true), "must park: {r}");

    // Before the deadline, the op stays pending.
    let _ = proj.snapshot(1_002);
    let summaries = proj.with_inner(|h| h.pending_op_summaries()).unwrap();
    assert_eq!(
        summaries.len(),
        1,
        "op must still be pending before deadline: {summaries:?}"
    );

    // A snapshot PAST the deadline evicts it — NO KP was ever ingested.
    let expired_now = 1_001 + PENDING_OP_EXPIRY_SECS + 1;
    let snap = proj.snapshot(expired_now);
    let summaries = proj.with_inner(|h| h.pending_op_summaries()).unwrap();
    assert!(
        summaries.is_empty(),
        "op must expire on the snapshot edge with no further ingest: {summaries:?}"
    );
    assert!(snap.groups.is_empty(), "no group must be created: {snap:?}");

    // Exactly one terminal FAILURE was recorded under the original id.
    let cmds = proj.with_inner(|h| h.drain_captured_commands()).unwrap();
    assert_eq!(cmds.len(), 1, "exactly one terminal command: {cmds:?}");
    assert_eq!(cmds[0].0, "failure", "verdict must be failure: {cmds:?}");
    assert_eq!(
        cmds[0].1, "corr-snap-expiry",
        "under the original correlation_id: {cmds:?}"
    );
}

/// Assert exactly one terminal command per correlation id across retry,
/// expiry, and late-KP-after-expiry outcomes.
#[test]
fn exactly_one_terminal_command_per_correlation_id() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate(); // retry-success peer
    let carol_keys = Keys::generate(); // expiry peer (KP never arrives in time)

    let proj = MarmotProjection::new(in_memory(alice_keys.clone()), None);
    proj.with_inner(|h| {
        ops::dispatch_json_for_tests(
            h,
            json!({ "op": "publish_key_package", "relays": ["wss://t.relay"] }),
            1_000,
            None,
        )
    })
    .unwrap();

    // (A) retry-success op blocked on Bob.
    proj.with_inner(|h| {
        ops::dispatch_json_for_tests(
            h,
            json!({
                "op": "create_group",
                "name": "Retry Success",
                "relays": ["wss://t.relay"],
                "invitee_npubs": [bob_keys.public_key().to_hex()],
            }),
            1_001,
            Some("corr-success"),
        )
    })
    .unwrap();

    // (B) expiry op blocked on Carol.
    proj.with_inner(|h| {
        ops::dispatch_json_for_tests(
            h,
            json!({
                "op": "create_group",
                "name": "Will Expire",
                "relays": ["wss://t.relay"],
                "invitee_npubs": [carol_keys.public_key().to_hex()],
            }),
            1_001,
            Some("corr-expire"),
        )
    })
    .unwrap();

    // Bob's KP arrives in-window → corr-success completes (one `success`).
    let bob_kp = make_kp_event(&bob_keys);
    proj.with_inner(|h| ingest_signed_event_core(h, &bob_kp, 1_002))
        .unwrap()
        .unwrap();

    // A snapshot past Carol's deadline expires corr-expire (one `failure`).
    let expired_now = 1_001 + PENDING_OP_EXPIRY_SECS + 1;
    let _ = proj.snapshot(expired_now);

    // (C) expiry-then-late-KP: Carol's KP finally arrives AFTER expiry. It
    // must NOT produce a second terminal verdict for corr-expire (the op is
    // already gone from the store).
    let carol_kp = make_kp_event(&carol_keys);
    proj.with_inner(|h| ingest_signed_event_core(h, &carol_kp, expired_now + 1))
        .unwrap()
        .unwrap();

    // Drain and assert: exactly one terminal per correlation_id, no duplicates.
    let cmds = proj.with_inner(|h| h.drain_captured_commands()).unwrap();
    assert_eq!(
        cmds.len(),
        2,
        "exactly two terminal commands total (one per op): {cmds:?}"
    );
    let success_count = cmds
        .iter()
        .filter(|(v, c)| *v == "success" && c == "corr-success")
        .count();
    let failure_count = cmds
        .iter()
        .filter(|(v, c)| *v == "failure" && c == "corr-expire")
        .count();
    assert_eq!(
        success_count, 1,
        "exactly one success for corr-success: {cmds:?}"
    );
    assert_eq!(
        failure_count, 1,
        "exactly one failure for corr-expire: {cmds:?}"
    );
    // No correlation_id appears twice.
    let mut ids: Vec<&String> = cmds.iter().map(|(_, c)| c).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(
        ids.len(),
        2,
        "no correlation_id may receive two terminals: {cmds:?}"
    );
}
