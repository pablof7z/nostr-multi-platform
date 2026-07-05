//! Regression coverage for chirp#129: `MarmotAction::CreateGroup` driven
//! through the FULL `ops::dispatch` orchestration path (not just the bare
//! `MarmotService` API `round_trip.rs` already covers).
//!
//! Every existing Marmot test in this crate (and in
//! `nmp-testing/tests/marmot_*.rs`) drives `MarmotService::create_group`
//! directly — never `ops::dispatch(&MarmotAction::CreateGroup {..})` through
//! a real `MarmotProjection`/`InnerHandle`. That orchestration layer (relay
//! resolution, the KP cache fill, the ingest-triggered parked-op retry) had
//! zero coverage before this file. This test drives the exact path a host
//! action dispatch takes: ingest the invitee's kind:30443 (populating the
//! cache the same way the kernel's `MarmotIngestParser` does), THEN dispatch
//! `CreateGroup`, proving it reaches a terminal `Ok`/`Err` — not an eternal
//! hang — once the invitee's key package is available.

use serde_json::json;

use crate::projection::action::MarmotAction;
use crate::projection::ops;
use crate::projection::state::MarmotProjection;
use crate::service::MarmotService;
use mdk_sqlite_storage::MdkSqliteStorage;
use nostr::nips::nip19::ToBech32;
use nostr::Keys;

fn in_memory_projection(keys: Keys) -> MarmotProjection {
    let storage = MdkSqliteStorage::new_in_memory().expect("in-memory mls storage");
    let service = MarmotService::from_storage(storage, keys, Default::default());
    MarmotProjection::new(service, None)
}

/// End-to-end: bob publishes a key package; alice's projection ingests
/// bob's signed kind:30443 event (the exact call `MarmotIngestParser` makes);
/// alice then dispatches `CreateGroup` naming bob as the sole invitee.
///
/// The dispatch MUST terminate with `ok:true` and a `group_id_hex` — it must
/// NOT hang and must NOT re-park (`pending:true`) once the invitee's KP is
/// already cached, reproducing the exact "Waiting for key packages" ->
/// "Creating..." transition chirp#129 reports getting stuck on.
#[test]
fn create_group_dispatch_completes_once_invitee_kp_is_cached() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let alice_proj = in_memory_projection(alice_keys.clone());
    let bob_service = {
        let storage = MdkSqliteStorage::new_in_memory().expect("bob mls storage");
        MarmotService::from_storage(storage, bob_keys.clone(), Default::default())
    };

    // Bob publishes his key package (kind:30443).
    let bob_kp = bob_service
        .publish_key_package(vec![nostr::RelayUrl::parse("wss://test.relay").unwrap()])
        .expect("bob publishes key package");

    // Simulate the kernel delivering bob's kind:30443 to alice's ingest
    // parser — the exact call `MarmotIngestParser::parse_at_source` makes
    // (crate::projection::tap), landing in `ops::ingest_signed_event_core`.
    let ingest_result = alice_proj
        .with_inner(|h| ops::ingest_signed_event_core(h, &bob_kp.event_30443, 1_000))
        .expect("projection lock available");
    assert!(
        ingest_result.is_ok(),
        "ingesting bob's kind:30443 must not error: {ingest_result:?}"
    );

    // Now dispatch CreateGroup naming bob — his KP is already cached, so this
    // MUST proceed straight to real group creation (no park, no hang).
    let action: MarmotAction = serde_json::from_value(json!({
        "op": "create_group",
        "name": "chirp-129 regression",
        "description": "",
        "invitee_npubs": [bob_keys.public_key().to_bech32().unwrap()],
        "relays": ["wss://test.relay"],
    }))
    .expect("valid CreateGroup action json");

    let result = alice_proj
        .with_inner(|h| ops::dispatch(h, &action, 1_001, Some("corr-129")))
        .expect("projection lock available");

    assert_eq!(
        result.get("pending").and_then(serde_json::Value::as_bool),
        None,
        "must not re-park when the invitee KP is already cached: {result}"
    );
    assert_eq!(
        result.get("ok").and_then(serde_json::Value::as_bool),
        Some(true),
        "create_group dispatch must terminate ok once the invitee KP is available: {result}"
    );
    assert!(
        result
            .get("group_id_hex")
            .and_then(serde_json::Value::as_str)
            .is_some(),
        "successful create_group must return a group_id_hex: {result}"
    );
}
