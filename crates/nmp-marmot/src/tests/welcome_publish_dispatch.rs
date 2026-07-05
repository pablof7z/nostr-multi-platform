//! #3057 ROUND 5 — PUBLISH-side: does nmp-marmot actually DISPATCH the
//! kind:1059 Welcome to the publish port?
//!
//! On-device tracing overturned the ingest hypothesis: B's ingest is active,
//! but A (the inviter) never puts a kind:1059 on the wire. MDK logs "Decoded
//! key package" then "Encoded welcome using base64" — then nothing. This test
//! isolates the nmp-marmot publish path: it drives `create_group` through the
//! real `ops::dispatch` with a RECORDING port and asserts a signed kind:1059
//! gift-wrap is handed to `publish_signed_explicit` with a non-empty relay pin
//! and the `VerifiedPrivateInbox` route class.
//!
//! If the recording port captures NO kind:1059, the drop is on the nmp-marmot
//! side (empty rumors / resolve returns empty / wrap not called). If it DOES,
//! the nmp-marmot side is correct and the drop is kernel-side (the host test in
//! `nmp-testing` covers that).

use std::collections::HashMap;
use std::sync::Mutex;

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

/// A `MarmotRuntimePort` that RECORDS every `publish_signed_explicit` call so
/// the test can assert exactly what nmp-marmot dispatched to the publish layer.
#[derive(Default)]
struct RecordingPort {
    dm_inboxes: HashMap<String, Vec<String>>,
    published: Mutex<Vec<(u16, Vec<String>, nmp_core::publish::PublishRouteClass)>>,
}

impl RecordingPort {
    fn published(&self) -> Vec<(u16, Vec<String>, nmp_core::publish::PublishRouteClass)> {
        self.published.lock().expect("publish lock").clone()
    }
}

impl MarmotRuntimePort for RecordingPort {
    fn publish_signed_explicit(
        &self,
        event: &nostr::Event,
        relays: &[nostr::RelayUrl],
        route_class: nmp_core::publish::PublishRouteClass,
    ) {
        self.published.lock().expect("publish lock").push((
            event.kind.as_u16(),
            relays.iter().map(std::string::ToString::to_string).collect(),
            route_class,
        ));
    }

    fn ensure_interest(
        &self,
        _identity: nmp_core::subs::SubIdentity,
        _interest: nmp_planner::LogicalInterest,
    ) {
    }

    fn write_relay_urls(&self, _author_hex: &str, _kind: u32) -> Vec<String> {
        Vec::new()
    }

    fn send_actor_command(&self, _cmd: nmp_core::actor::ActorCommand) {}

    fn dm_inbox_relays(&self, pubkey_hex: &str) -> Option<Vec<String>> {
        self.dm_inboxes.get(pubkey_hex).cloned()
    }
}

/// A `create_group` inviting Bob MUST dispatch a signed kind:1059 gift-wrap
/// Welcome to Bob's resolved kind:10050 DM-inbox relays under the
/// `VerifiedPrivateInbox` route class. If nothing is dispatched, A's Welcome
/// dies inside nmp-marmot (the #3057 round-5 publish-side drop).
#[test]
fn create_group_dispatches_a_kind1059_welcome_to_the_publish_port() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let alice_proj = in_memory_projection(alice_keys.clone());
    let bob_service = {
        let storage = MdkSqliteStorage::new_in_memory().expect("bob mls storage");
        MarmotService::from_storage(storage, bob_keys.clone(), Default::default())
    };
    let bob_kp = bob_service
        .publish_key_package(vec![nostr::RelayUrl::parse("wss://test.relay").unwrap()])
        .expect("bob publishes key package");

    // Bob's kind:10050 DM inbox is RESOLVED (non-empty) — the precondition for
    // an honest VerifiedPrivateInbox Welcome publish.
    let port = RecordingPort {
        dm_inboxes: HashMap::from([(
            bob_keys.public_key().to_hex(),
            vec!["wss://bob-dm-inbox.example".to_string()],
        )]),
        ..Default::default()
    };

    // Ingest Bob's kind:30443 into Alice's projection (populates the KP cache).
    let ingest = alice_proj
        .with_inner_port(&port, |h| {
            ops::ingest_signed_event_core(h, &bob_kp.event_30443, 1_000)
        })
        .expect("projection lock");
    assert!(ingest.is_ok(), "ingesting bob's KP must not error: {ingest:?}");

    // Dispatch create_group inviting Bob.
    let action: MarmotAction = serde_json::from_value(json!({
        "op": "create_group",
        "name": "round5 publish",
        "description": "",
        "invitee_npubs": [bob_keys.public_key().to_bech32().unwrap()],
        "relays": ["wss://test.relay"],
    }))
    .expect("valid CreateGroup action");

    let result = alice_proj
        .with_inner_port(&port, |h| ops::dispatch(h, &action, 1_001, Some("r5")))
        .expect("projection lock");

    assert_eq!(
        result.get("ok").and_then(serde_json::Value::as_bool),
        Some(true),
        "create_group must succeed: {result}"
    );

    // THE LOAD-BEARING ASSERTION: a kind:1059 was dispatched to the publish
    // port, to Bob's DM-inbox relay, under VerifiedPrivateInbox.
    let published = port.published();
    let welcome = published
        .iter()
        .find(|(kind, _, _)| *kind == 1059)
        .unwrap_or_else(|| {
            panic!(
                "create_group must dispatch a kind:1059 Welcome to the publish port; \
                 dispatched kinds = {:?}. If empty/absent, A's Welcome is dropped inside \
                 nmp-marmot before it reaches the wire (the #3057 round-5 bug).",
                published.iter().map(|(k, _, _)| *k).collect::<Vec<_>>()
            )
        });

    assert!(
        welcome.1.contains(&"wss://bob-dm-inbox.example".to_string()),
        "the kind:1059 must be pinned to Bob's resolved kind:10050 DM-inbox relay; got {:?}",
        welcome.1
    );
    assert_eq!(
        welcome.2,
        nmp_core::publish::PublishRouteClass::VerifiedPrivateInbox,
        "the kind:1059 Welcome must claim the VerifiedPrivateInbox route class (D10)"
    );
}
