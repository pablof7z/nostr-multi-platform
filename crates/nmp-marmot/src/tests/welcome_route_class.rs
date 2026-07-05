//! Regression coverage for pablof7z/nostr-multi-platform#3053: a Marmot
//! group invite's kind:1059 Welcome must publish through the ONE route
//! class D10 accepts for private kinds (`VerifiedPrivateInbox`), routed to
//! the invitee's own resolved kind:10050 DM-inbox relays — never the
//! group's relays, and never the previously-hardcoded `ImportedOrPresigned`
//! (which D10 unconditionally rejects for kind:1059, so no invite could
//! ever be delivered — nak-confirmed zero kind:1059 ever reached a relay).
//!
//! Also pins the compounding ordering bug: a `create_group` that cannot
//! honestly earn `VerifiedPrivateInbox` for an invitee (their DM inbox is
//! not yet resolved) must fail BEFORE the MLS roster is mutated, so a
//! failed invite is cleanly retryable instead of a permanent phantom
//! member (`mdk error: Duplicate signature key in proposals and group`).

use std::cell::RefCell;
use std::collections::HashMap;

use serde_json::json;

use crate::projection::action::MarmotAction;
use crate::projection::ops;
use crate::projection::state::{MarmotProjection, MarmotRuntimePort};
use crate::service::MarmotService;
use mdk_sqlite_storage::MdkSqliteStorage;
use nostr::{JsonUtil, Keys};

fn in_memory_projection(keys: Keys) -> MarmotProjection {
    let storage = MdkSqliteStorage::new_in_memory().expect("in-memory mls storage");
    let service = MarmotService::from_storage(storage, keys, Default::default());
    MarmotProjection::new(service, None)
}

/// One captured `publish_signed_explicit` call: `(kind, relays, route_class)`.
type CapturedPublish = (u16, Vec<String>, nmp_core::publish::PublishRouteClass);

/// A `MarmotRuntimePort` stub whose `dm_inboxes` map is mutable through a
/// shared reference (`RefCell`) so a test can flip an invitee's resolved
/// inbox between two `dispatch` calls on the SAME projection — modeling the
/// kernel resolving a peer's kind:10050 list between a failed attempt and a
/// retry. Every `publish_signed_explicit` call is recorded for assertion.
#[derive(Default)]
struct FakePort {
    dm_inboxes: RefCell<HashMap<String, Vec<String>>>,
    publishes: RefCell<Vec<CapturedPublish>>,
}

impl FakePort {
    fn set_dm_inbox(&self, pubkey_hex: &str, relays: Vec<String>) {
        self.dm_inboxes
            .borrow_mut()
            .insert(pubkey_hex.to_string(), relays);
    }
}

impl MarmotRuntimePort for FakePort {
    fn publish_signed_explicit(
        &self,
        event: &nostr::Event,
        relays: &[nostr::RelayUrl],
        route_class: nmp_core::publish::PublishRouteClass,
    ) {
        self.publishes.borrow_mut().push((
            event.kind.as_u16(),
            relays
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
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
        self.dm_inboxes.borrow().get(pubkey_hex).cloned()
    }
}

fn bob_key_package(bob_keys: &Keys) -> nostr::Event {
    let storage = MdkSqliteStorage::new_in_memory().expect("bob mls storage");
    let bob_service = MarmotService::from_storage(storage, bob_keys.clone(), Default::default());
    bob_service
        .publish_key_package(vec![nostr::RelayUrl::parse("wss://bob.kp.relay").unwrap()])
        .expect("bob publishes key package")
        .event_30443
}

fn create_group_action(bob_kp_json: &str) -> MarmotAction {
    serde_json::from_value(json!({
        "op": "create_group",
        "name": "route-class regression",
        "description": "",
        "relays": ["wss://group.relay"],
        "signed_key_package_events_json": [bob_kp_json],
    }))
    .expect("valid CreateGroup action json")
}

/// #3053 root cause: before the fix, the Welcome's `publish_signed_explicit`
/// call unconditionally claimed `ImportedOrPresigned` and routed to the
/// GROUP's relays — a route D10 rejects for kind:1059, so the invite could
/// never be delivered. The fix routes to the invitee's own resolved
/// kind:10050 inbox under `VerifiedPrivateInbox`, the one class D10 accepts.
#[test]
fn create_group_welcome_uses_verified_private_inbox_to_invitees_own_inbox() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();
    let bob_kp = bob_key_package(&bob_keys);
    let bob_kp_json = bob_kp.as_json();
    let bob_hex = bob_keys.public_key().to_hex();

    let proj = in_memory_projection(alice_keys);
    let port = FakePort::default();
    port.set_dm_inbox(&bob_hex, vec!["wss://bob-inbox.example".to_string()]);

    let action = create_group_action(&bob_kp_json);
    let result = proj
        .with_inner_port(&port, |h| ops::dispatch(h, &action, 1_000, Some("corr")))
        .expect("projection lock available");

    assert_eq!(
        result.get("ok").and_then(serde_json::Value::as_bool),
        Some(true),
        "create_group must succeed once the invitee's DM inbox is resolved: {result}"
    );

    let publishes = port.publishes.borrow();
    let welcome = publishes
        .iter()
        .find(|(kind, ..)| *kind == 1059)
        .expect("a kind:1059 Welcome must have been published");
    assert_eq!(
        welcome.2,
        nmp_core::publish::PublishRouteClass::VerifiedPrivateInbox,
        "the Welcome MUST claim VerifiedPrivateInbox — D10 rejects kind:1059 \
         under any other route class, which is exactly why every invite \
         failed before the fix"
    );
    assert_eq!(
        welcome.1,
        vec!["wss://bob-inbox.example".to_string()],
        "the Welcome MUST route to the invitee's OWN resolved DM inbox, \
         not the group's relays (the pre-fix inbox-routing approximation)"
    );
}

/// Compounding bug: a `create_group` that cannot honestly earn
/// `VerifiedPrivateInbox` for an invitee (their DM inbox is not yet
/// resolved) must fail WITHOUT mutating the local MLS roster, so a retry
/// after the inbox resolves succeeds cleanly instead of hitting `mdk error:
/// Duplicate signature key in proposals and group` against a phantom member.
#[test]
fn failed_welcome_publish_leaves_no_phantom_member_and_retry_recovers() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();
    let bob_kp = bob_key_package(&bob_keys);
    let bob_kp_json = bob_kp.as_json();
    let bob_hex = bob_keys.public_key().to_hex();

    let proj = in_memory_projection(alice_keys);
    let port = FakePort::default();
    // Bob's inbox is NOT yet resolved.

    let action = create_group_action(&bob_kp_json);
    let first = proj
        .with_inner_port(&port, |h| ops::dispatch(h, &action, 1_000, Some("corr-1")))
        .expect("projection lock available");
    assert_eq!(
        first.get("ok").and_then(serde_json::Value::as_bool),
        Some(false),
        "create_group must fail closed when the invitee's DM inbox is \
         unresolved, not silently proceed: {first}"
    );
    assert!(
        port.publishes.borrow().is_empty(),
        "no kind:1059 Welcome (or any event) may be published when the \
         invitee's inbox cannot be resolved"
    );
    // No group was created — the MLS roster was never touched.
    assert!(
        proj.snapshot(1_000).groups.is_empty(),
        "a failed create_group must leave zero groups (no phantom roster \
         mutation), proving the ordering fix resolves the invitee's inbox \
         BEFORE `service.create_group` runs"
    );

    // Bob's inbox resolves; retry the SAME action.
    port.set_dm_inbox(&bob_hex, vec!["wss://bob-inbox.example".to_string()]);
    let second = proj
        .with_inner_port(&port, |h| ops::dispatch(h, &action, 1_001, Some("corr-2")))
        .expect("projection lock available");
    assert_eq!(
        second.get("ok").and_then(serde_json::Value::as_bool),
        Some(true),
        "retrying create_group after the inbox resolves must succeed \
         cleanly — no leftover phantom-member state from the failed \
         attempt: {second}"
    );
    let snapshot = proj.snapshot(1_001);
    assert_eq!(
        snapshot.groups.len(),
        1,
        "the retried create_group must produce exactly one group"
    );
    assert_eq!(
        snapshot.groups[0].member_count, 2,
        "the group must have exactly alice + bob — no duplicate/phantom member"
    );
}
