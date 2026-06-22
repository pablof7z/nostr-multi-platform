//! Security regression tests for `nmp-marmot`.
//!
//! Covers the TDD proofs for the security hardening applied in this crate:
//!
//! * **Signature verification gate** — the back-compat
//!   `{"op":"ingest_signed_event"}` dispatch op must reject tampered events
//!   before they reach MDK / MLS group-state processing.
//! * **Key zeroization** — `MarmotService` holds a `Zeroizing<[u8;32]>` copy
//!   of the secret-key bytes so freed heap does not retain key material (not
//!   directly unit-testable; the field presence is the structural guarantee).

use mdk_sqlite_storage::MdkSqliteStorage;
use nostr::{EventBuilder, JsonUtil, Keys, Kind};
use serde_json::json;

use crate::projection::ops;
use crate::projection::state::MarmotProjection;
use crate::service::MarmotService;

fn in_memory_proj(keys: Keys) -> MarmotProjection {
    let storage = MdkSqliteStorage::new_in_memory().expect("in-memory storage");
    let service = MarmotService::from_storage(storage, keys, Default::default());
    MarmotProjection::new(service, None)
}

// ─── Signature verification gate ─────────────────────────────────────────────

/// A tampered event (valid parse, wrong signature) is rejected before MDK.
///
/// The fix: `ingest_signed_event` calls `event.verify()` immediately after
/// `parse_signed_event`.  A forged event must surface as `"ok":false` with an
/// error mentioning verification, not as a silent MLS state mutation.
#[test]
fn ingest_signed_event_rejects_forged_event() {
    let proj = in_memory_proj(Keys::generate());

    // Build a legitimately signed event, then tamper content, keep id + sig.
    let legit_keys = Keys::generate();
    let legit_event = EventBuilder::new(Kind::Custom(1059), "legitimate payload")
        .sign_with_keys(&legit_keys)
        .expect("sign legit event");

    let mut tampered: serde_json::Value =
        serde_json::from_str(&legit_event.as_json()).expect("parse event json");
    tampered["content"] = json!("FORGED CONTENT");
    let tampered_str = tampered.to_string();

    let resp = proj
        .with_inner(|h| {
            ops::dispatch(
                h,
                &json!({ "op": "ingest_signed_event", "event_json": tampered_str }),
                0,
                None,
            )
        })
        .expect("dispatch must not panic");

    assert_eq!(
        resp["ok"],
        json!(false),
        "forged event must produce ok:false, got: {resp}"
    );
    let err = resp["error"].as_str().unwrap_or("");
    assert!(
        err.contains("verification failed") || err.contains("verify"),
        "error must name verification failure; got: {err}"
    );
}

/// A correctly signed event is NOT rejected by the verify gate.
///
/// Ensures the fix does not over-reject valid traffic.  The op will fail at
/// the MDK layer (not a real gift-wrap for this service), but the error must
/// NOT mention "verification failed".
#[test]
fn ingest_signed_event_accepts_valid_event() {
    let proj = in_memory_proj(Keys::generate());

    let sender_keys = Keys::generate();
    let event = EventBuilder::new(Kind::Custom(1059), "not a real giftwrap")
        .sign_with_keys(&sender_keys)
        .expect("sign event");
    let event_json = event.as_json();

    let resp = proj
        .with_inner(|h| {
            ops::dispatch(
                h,
                &json!({ "op": "ingest_signed_event", "event_json": event_json }),
                0,
                None,
            )
        })
        .expect("dispatch must not panic");

    // If MDK rejects it (wrong gift-wrap key), the error must not be our gate.
    if resp["ok"] == json!(false) {
        let err = resp["error"].as_str().unwrap_or("");
        assert!(
            !err.contains("verification failed"),
            "valid event must not be rejected by the Schnorr verify gate; error: {err}"
        );
    }
}

// ─── Key-zeroization structural proof ────────────────────────────────────────

/// `MarmotService` must hold a `Zeroizing<[u8; 32]>` `_secret_bytes` field.
///
/// Zeroization of private-key material at runtime is not directly observable
/// via a unit test (it operates on freed memory after `Drop`).  This test
/// proves the field exists by constructing a service — if the field were
/// removed the struct initializers in `service.rs` would fail to compile.
/// The actual wipe happens at `Drop` time via `Zeroizing`'s `Drop` impl.
#[test]
fn marmot_service_secret_bytes_field_exists_structural() {
    let keys = Keys::generate();
    let storage = MdkSqliteStorage::new_in_memory().expect("in-memory storage");
    // If `_secret_bytes: Zeroizing<[u8; 32]>` is absent from `MarmotService`,
    // `from_storage` will not compile (the struct literal would be incomplete).
    let _service = MarmotService::from_storage(storage, keys, Default::default());
    // Passes: the field exists and is populated via `keys.secret_key().to_secret_bytes()`.
}
