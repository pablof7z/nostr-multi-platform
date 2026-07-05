//! #3057 ROUND 7 — key-package SELECTION freshness.
//!
//! On-device: A builds the Welcome against a STALE kind:30443 from B's relay
//! history (6 accumulated key packages, distinct `d` tags), so B's
//! `process_welcome` fails "No matching key package was found in the key
//! store" — only the newest key package matches the private half in B's live
//! MLS store. This drives `create_group` through the real `ops::dispatch` with
//! BOTH a stale and a fresh key package cached for B, asserts A selects the
//! FRESH one, and that B (its live store) can actually process the resulting
//! Welcome.

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::{json, Value};

use crate::projection::action::MarmotAction;
use crate::projection::ops;
use crate::projection::state::{MarmotProjection, MarmotRuntimePort};
use crate::service::MarmotService;
use mdk_sqlite_storage::MdkSqliteStorage;
use nostr::nips::nip19::ToBech32;
use nostr::{EventBuilder, Keys, Kind, Timestamp};

fn in_memory_service(keys: Keys) -> MarmotService {
    let storage = MdkSqliteStorage::new_in_memory().expect("in-memory mls storage");
    MarmotService::from_storage(storage, keys, Default::default())
}

fn in_memory_projection(keys: Keys) -> MarmotProjection {
    MarmotProjection::new(in_memory_service(keys), None)
}

/// Re-stamp a kind:30443 with a specific `created_at`, re-signed by `keys`. The
/// embedded MLS key package (in `content`) is unchanged, so the init-key the
/// event advertises — and therefore which store holds its private half — is
/// preserved; only the freshness ordering changes.
fn restamp_key_package(event: &nostr::Event, keys: &Keys, created_at: u64) -> nostr::Event {
    EventBuilder::new(Kind::Custom(30443), event.content.clone())
        .tags(event.tags.clone())
        .custom_created_at(Timestamp::from(created_at))
        .sign_with_keys(keys)
        .expect("re-sign kind:30443")
}

#[derive(Default)]
struct RecordingPort {
    dm_inboxes: HashMap<String, Vec<String>>,
    published: Mutex<Vec<nostr::Event>>,
}

impl MarmotRuntimePort for RecordingPort {
    fn publish_signed_explicit(
        &self,
        event: &nostr::Event,
        _relays: &[nostr::RelayUrl],
        _route_class: nmp_core::publish::PublishRouteClass,
    ) {
        self.published.lock().expect("lock").push(event.clone());
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

/// A must build the Welcome against B's CURRENT (newest) key package, even when
/// a STALE one for B was ingested LAST — so B's live store can process it.
///
/// FAILS on master: the cache keeps the last-ingested key package (stale here),
/// so A builds the Welcome against it and B's `unwrap_and_process_welcome`
/// errors "No matching key package". PASSES with the round-7 fix (keep/select
/// newest by `created_at`).
#[test]
fn create_group_builds_welcome_against_the_freshest_key_package_b_can_process() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();
    let bob_hex = bob_keys.public_key().to_hex();

    // B's CURRENT store — holds the private half of the FRESH key package.
    let bob_current = in_memory_service(bob_keys.clone());
    let fresh_raw = bob_current
        .publish_key_package(vec![nostr::RelayUrl::parse("wss://test.relay").unwrap()])
        .expect("bob fresh kp");
    // A STALE key package from a prior B store instance — B's current store does
    // NOT hold its private half. Newer/older ordering is set by created_at.
    let bob_old = in_memory_service(bob_keys.clone());
    let stale_raw = bob_old
        .publish_key_package(vec![nostr::RelayUrl::parse("wss://test.relay").unwrap()])
        .expect("bob stale kp");

    let fresh = restamp_key_package(&fresh_raw.event_30443, &bob_keys, 2_000);
    let stale = restamp_key_package(&stale_raw.event_30443, &bob_keys, 1_000);
    assert_ne!(fresh.id, stale.id, "fresh and stale must be distinct events");

    let alice_proj = in_memory_projection(alice_keys.clone());
    let port = RecordingPort {
        dm_inboxes: HashMap::from([(bob_hex.clone(), vec!["wss://bob-dm.example".to_string()])]),
        ..Default::default()
    };

    // Ingest FRESH first, then STALE LAST — so the pre-fix "last-ingested wins"
    // cache would hold the STALE one (the exact on-device failure shape).
    for kp in [&fresh, &stale] {
        alice_proj
            .with_inner_port(&port, |h| ops::ingest_signed_event_core(h, kp, 1_000))
            .expect("lock")
            .expect("kp ingest ok");
    }

    let action: MarmotAction = serde_json::from_value(json!({
        "op": "create_group",
        "name": "round7",
        "description": "",
        "invitee_npubs": [bob_keys.public_key().to_bech32().unwrap()],
        "relays": ["wss://test.relay"],
    }))
    .expect("valid action");

    let result = alice_proj
        .with_inner_port(&port, |h| ops::dispatch(h, &action, 1_001, Some("r7")))
        .expect("lock");
    assert_eq!(
        result.get("ok").and_then(Value::as_bool),
        Some(true),
        "create_group must succeed: {result}"
    );

    // A published exactly one kind:1059 Welcome. Feed it to B's LIVE store.
    let published = port.published.lock().expect("lock").clone();
    let welcome_gift = published
        .iter()
        .find(|e| e.kind == Kind::from_u16(1059))
        .expect("a kind:1059 Welcome must be published")
        .clone();

    // THE LOAD-BEARING ASSERTION: B's current store can process the Welcome —
    // proving A selected the FRESH key package (whose private half B holds), not
    // the stale one. On master this errs "No matching key package".
    let processed = bob_current.unwrap_and_process_welcome(&welcome_gift);
    assert!(
        processed.is_ok(),
        "B's live store must process the Welcome A built — A must select the \
         FRESH key package, not a stale one from relay history. got: {:?}",
        processed.err()
    );
}
