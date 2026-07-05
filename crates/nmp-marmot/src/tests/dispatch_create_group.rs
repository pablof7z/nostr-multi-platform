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

use std::collections::HashMap;

use serde_json::json;

use crate::projection::action::MarmotAction;
use crate::projection::ops;
use crate::projection::state::{MarmotProjection, MarmotRuntimePort};
use crate::service::MarmotService;
use mdk_sqlite_storage::MdkSqliteStorage;
use nostr::nips::nip19::ToBech32;
use nostr::Keys;

fn in_memory_projection(keys: Keys) -> MarmotProjection {
    let storage = MdkSqliteStorage::new_in_memory().expect("in-memory mls storage");
    let service = MarmotService::from_storage(storage, keys, Default::default());
    MarmotProjection::new(service, None)
}

/// Minimal `MarmotRuntimePort` stub for driving `ops::dispatch` end-to-end
/// without a live actor/kernel. Publishing + interest registration are
/// no-ops (this file asserts dispatch *terminates*, not relay traffic);
/// `dm_inbox_relays` is the one method with real behavior — it stands in
/// for the kernel's resolved kind:10050 cache so a `create_group` inviting
/// a peer can earn D10's `VerifiedPrivateInbox` route class for the
/// Welcome exactly as production does.
#[derive(Default)]
struct FakePort {
    dm_inboxes: HashMap<String, Vec<String>>,
}

impl MarmotRuntimePort for FakePort {
    fn publish_signed_explicit(
        &self,
        _event: &nostr::Event,
        _relays: &[nostr::RelayUrl],
        _route_class: nmp_core::publish::PublishRouteClass,
    ) {
    }

    fn ensure_interest(
        &self,
        _identity: nmp_core::subs::SubIdentity,
        _interest: nmp_planner::LogicalInterest,
    ) {
    }

    fn write_relay_urls(&self, _author_hex: &str, _kind: u32) -> Vec<String> {
        // Empty → `resolve_write_relays` falls back to the envelope's own
        // `relays` field, same as the previous no-port (`with_inner`) path.
        Vec::new()
    }

    fn send_actor_command(&self, _cmd: nmp_core::actor::ActorCommand) {}

    fn dm_inbox_relays(&self, pubkey_hex: &str) -> Option<Vec<String>> {
        self.dm_inboxes.get(pubkey_hex).cloned()
    }
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

    // D10: the Welcome can only earn `VerifiedPrivateInbox` once bob's
    // kind:10050 DM-inbox relays are resolved — mirror that here exactly as
    // the kernel would after resolving bob's inbox.
    let port = FakePort {
        dm_inboxes: HashMap::from([(
            bob_keys.public_key().to_hex(),
            vec!["wss://bob-inbox.example".to_string()],
        )]),
    };

    // Simulate the kernel delivering bob's kind:30443 to alice's ingest
    // parser — the exact call `MarmotIngestParser::parse_at_source` makes
    // (crate::projection::tap), landing in `ops::ingest_signed_event_core`.
    let ingest_result = alice_proj
        .with_inner_port(&port, |h| {
            ops::ingest_signed_event_core(h, &bob_kp.event_30443, 1_000)
        })
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
        .with_inner_port(&port, |h| {
            ops::dispatch(h, &action, 1_001, Some("corr-129"))
        })
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
