//! Snapshot-surface tests for deferred-op state (PR-2 — marmot-create-fix).
//!
//! Proves the `MarmotSnapshot.pending_ops` rows (with `age_secs` temporal
//! context) and the `last_op_error` banner are REAL production-set values:
//! a parked op surfaces as a `pending_ops` row and clears on retry; an
//! expired op populates `last_op_error` with the real failure context and a
//! later success clears it.

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

fn make_kp_event(keys: &Keys) -> nostr::Event {
    let relay = nostr::RelayUrl::parse("wss://t.relay").unwrap();
    in_memory(keys.clone())
        .publish_key_package(vec![relay])
        .expect("kp")
        .event_30443
        .clone()
}

/// While a create_group op is parked the snapshot surfaces a `pending_ops` row
/// with `missing_count` + `age_secs` (elapsed wait). Once the KP arrives and
/// the op completes, `pending_ops` must be empty.
#[test]
fn pending_op_appears_in_snapshot_and_clears_after_retry() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let proj = MarmotProjection::new(in_memory(alice_keys.clone()), true);
    proj.with_inner(|h| {
        ops::dispatch(
            h,
            &json!({ "op": "publish_key_package", "relays": ["wss://t.relay"] }),
            1_000,
            None,
        )
    })
    .unwrap();

    // Park the op.
    proj.with_inner(|h| {
        ops::dispatch(
            h,
            &json!({
                "op": "create_group",
                "name": "Snapshot Test",
                "relays": ["wss://t.relay"],
                "invitee_npubs": [bob_keys.public_key().to_hex()],
            }),
            1_001,
            Some("corr-snap-1"),
        )
    })
    .unwrap();

    // Snapshot while pending (3 s after parking): one row with temporal context.
    let snap = proj.snapshot(1_004);
    assert_eq!(
        snap.pending_ops.len(),
        1,
        "pending op must appear in snapshot while parked: {snap:?}"
    );
    let row = &snap.pending_ops[0];
    assert_eq!(row.correlation_id, "corr-snap-1");
    assert_eq!(row.op_tag, "create_group");
    assert_eq!(row.missing_count, 1);
    assert_eq!(row.age_secs, 3, "age_secs = now 1_004 - parked 1_001: {row:?}");
    assert_eq!(
        row.missing_count, 1,
        "missing_count must equal the number of pending KPs: {row:?}"
    );

    // Ingest Bob's KP → retry fires → pending_ops must clear.
    let bob_kp = make_kp_event(&bob_keys);
    proj.with_inner(|h| ingest_signed_event_core(h, &bob_kp, 1_005))
        .unwrap()
        .unwrap();

    let snap = proj.snapshot(1_005);
    assert!(
        snap.pending_ops.is_empty(),
        "pending_ops must be empty after retry: {snap:?}"
    );
    assert_eq!(snap.groups.len(), 1, "group must appear after retry: {snap:?}");
}

/// `last_op_error` is a REAL production-set value, not dead wire. Full
/// lifecycle through the snapshot surface: park → expire via snapshot edge →
/// `last_op_error` populated with the real `(op, reason, correlation_id,
/// at_secs)` → successful op → cleared.
#[test]
fn last_op_error_is_set_on_expiry_and_cleared_on_next_success() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let proj = MarmotProjection::new(in_memory(alice_keys.clone()), true);
    proj.with_inner(|h| {
        ops::dispatch(
            h,
            &json!({ "op": "publish_key_package", "relays": ["wss://t.relay"] }),
            1_000,
            None,
        )
    })
    .unwrap();

    // No failure yet.
    assert!(
        proj.snapshot(1_000).last_op_error.is_none(),
        "no last_op_error before any op fails"
    );

    // Park an op that will never receive its KP.
    proj.with_inner(|h| {
        ops::dispatch(
            h,
            &json!({
                "op": "create_group",
                "name": "Will Expire",
                "relays": ["wss://t.relay"],
                "invitee_npubs": [bob_keys.public_key().to_hex()],
            }),
            1_001,
            Some("corr-will-expire"),
        )
    })
    .unwrap();

    // Still pending, no error yet.
    assert!(
        proj.snapshot(1_002).last_op_error.is_none(),
        "no error while the op is still pending"
    );

    // Snapshot PAST the deadline → expiry fires → last_op_error populated.
    let expired_now = 1_001 + PENDING_OP_EXPIRY_SECS + 1;
    let snap = proj.snapshot(expired_now);
    assert!(snap.pending_ops.is_empty(), "op evicted from pending: {snap:?}");
    let err = snap
        .last_op_error
        .clone()
        .expect("expiry must populate last_op_error");
    assert_eq!(err.op, "create_group", "op tag from the parked action: {err:?}");
    assert_eq!(err.reason, "key_package_unavailable");
    assert_eq!(err.correlation_id, "corr-will-expire");
    assert_eq!(err.at_secs, expired_now, "recorded at the expiry edge time");

    // A subsequent SUCCESSFUL op clears the banner (publish_key_package always
    // succeeds — no KP gating — so it is the cleanest clearing trigger).
    proj.with_inner(|h| {
        ops::dispatch(
            h,
            &json!({ "op": "publish_key_package", "relays": ["wss://t.relay"] }),
            expired_now + 1,
            None,
        )
    })
    .unwrap();

    assert!(
        proj.snapshot(expired_now + 1).last_op_error.is_none(),
        "a successful op must clear the last_op_error banner"
    );
}
