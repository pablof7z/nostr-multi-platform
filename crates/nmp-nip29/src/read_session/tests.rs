use std::any::Any;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{KernelEvent, ObservedProjection};
use nmp_core::{ObservedProjectionId, ObservedProjectionSink};
use nmp_ownership::ProjectionRegistrationKey;
use nmp_read_session::{
    DemandSetMembers, DemandSetReconciler, ReadHost, ReadOutputEncoder, ReadSessionBuild,
    ReadSessionId, ReadSessionRegistry, TeardownAction,
};

use super::*;
use crate::kinds::KIND_GROUP_MEMBERS;

const RELAY_A: &str = "wss://a.groups.example";
const RELAY_B: &str = "wss://b.groups.example";

struct FakeHost {
    registry: ReadSessionRegistry,
    log: Arc<Mutex<Vec<String>>>,
    next_interest: AtomicU64,
}

impl FakeHost {
    fn new() -> Self {
        Self {
            registry: ReadSessionRegistry::default(),
            log: Arc::new(Mutex::new(Vec::new())),
            next_interest: AtomicU64::new(1),
        }
    }

    fn live_count(&self) -> usize {
        self.registry.live_count()
    }

    fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }

    fn push(&self, entry: impl Into<String>) {
        self.log.lock().unwrap().push(entry.into());
    }
}

impl ReadHost for FakeHost {
    fn install_read_output(&self, key: ProjectionRegistrationKey, _encoder: ReadOutputEncoder) {
        self.push(format!("install:{}", key.as_str()));
    }

    fn open_read_interest(&self, decl: ObservedProjection) -> ObservedProjectionId {
        let id = self.next_interest.fetch_add(1, Ordering::Relaxed);
        self.push(format!(
            "open:{}:{}",
            decl.consumer_id,
            decl.relay_pin.unwrap_or_default()
        ));
        ObservedProjectionId(id)
    }

    fn teardown_close_interest(&self, id: ObservedProjectionId) -> TeardownAction {
        let log = Arc::clone(&self.log);
        Box::new(move || log.lock().unwrap().push(format!("close_interest:{}", id.0)))
    }

    fn teardown_remove_output(&self, key: String) -> TeardownAction {
        let log = Arc::clone(&self.log);
        Box::new(move || log.lock().unwrap().push(format!("remove_output:{key}")))
    }

    fn teardown_mark_changed(&self) -> TeardownAction {
        let log = Arc::clone(&self.log);
        Box::new(move || log.lock().unwrap().push("mark_changed".to_string()))
    }

    fn store_read_session(&self, build: ReadSessionBuild) -> ReadSessionId {
        self.registry.open(build)
    }

    fn read_session_projection_key(&self, id: &ReadSessionId) -> Option<String> {
        self.registry.projection_key(id)
    }

    fn close_read_session(&self, id: &ReadSessionId) -> bool {
        self.registry.close(id)
    }

    fn close_read_session_by_projection_key(&self, projection_key: &str) -> bool {
        self.registry.close_by_projection_key(projection_key)
    }

    fn read_session_id_for_projection_key(&self, projection_key: &str) -> Option<ReadSessionId> {
        self.registry.session_id_for_projection_key(projection_key)
    }

    fn read_demand_set_members(&self, projection_key: &str) -> Option<DemandSetMembers> {
        self.registry.demand_set_members(projection_key)
    }

    fn read_demand_set_reducer(&self, projection_key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        self.registry.demand_set_reducer(projection_key)
    }

    fn read_demand_set_reconciler(&self, projection_key: &str) -> Option<Arc<DemandSetReconciler>> {
        self.registry.demand_set_reconciler(projection_key)
    }
}

fn member_event(id: &str, relay: &str, room: &str, active_pubkey: &str) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: "relay".to_string(),
        kind: KIND_GROUP_MEMBERS,
        created_at: 100,
        tags: vec![
            vec!["d".to_string(), room.to_string()],
            vec!["p".to_string(), active_pubkey.to_string()],
        ],
        content: String::new(),
        relay_provenance: vec![relay.to_string()],
    }
}

fn open_count(log: &[String]) -> usize {
    log.iter()
        .filter(|entry| entry.starts_with("open:"))
        .count()
}

fn close_interest_count(log: &[String]) -> usize {
    log.iter()
        .filter(|entry| entry.starts_with("close_interest:"))
        .count()
}

#[test]
fn joined_groups_session_aggregates_active_pubkey_across_relay_set() {
    let host = FakeHost::new();
    let active = "a".repeat(64);
    let (handle, reader) = open_nip29_joined_groups_session_with_reader(
        &host,
        Nip29JoinedGroupsSession::new_for_relays(
            active.clone(),
            vec![RELAY_A.to_string(), RELAY_B.to_string()],
        ),
    )
    .expect("non-empty active pubkey opens joined-groups demand set");

    assert_eq!(host.live_count(), 1);
    assert_eq!(open_count(&host.log()), 2);

    reader.on_kernel_event(&member_event("a-members", RELAY_A, "room-a", &active));
    reader.on_kernel_event(&member_event("b-members", RELAY_B, "room-b", &active));

    let snapshot = reader.snapshot();
    assert_eq!(snapshot.groups.len(), 2);
    assert!(snapshot
        .groups
        .iter()
        .any(|group| group.group_id == "room-a" && group.host_relay_url == RELAY_A));
    assert!(snapshot
        .groups
        .iter()
        .any(|group| group.group_id == "room-b" && group.host_relay_url == RELAY_B));

    assert!(close_nip29_joined_groups_session(&host, handle));
    assert_eq!(host.live_count(), 0);
}

#[test]
fn joined_groups_reopen_with_same_pubkey_reconciles_relays_without_second_session() {
    let host = FakeHost::new();
    let active = "a".repeat(64);
    let (first, reader) = open_nip29_joined_groups_session_with_reader(
        &host,
        Nip29JoinedGroupsSession::new_for_relays(active.clone(), vec![RELAY_A.to_string()]),
    )
    .expect("joined-groups opens");
    reader.on_kernel_event(&member_event("a-members", RELAY_A, "room-a", &active));

    let (second, reconciled_reader) = open_nip29_joined_groups_session_with_reader(
        &host,
        Nip29JoinedGroupsSession::new_for_relays(
            active.clone(),
            vec![RELAY_A.to_string(), RELAY_B.to_string()],
        ),
    )
    .expect("joined-groups reconciles");

    assert!(Arc::ptr_eq(&reader, &reconciled_reader));
    assert_eq!(host.live_count(), 1);
    assert_eq!(open_count(&host.log()), 2);
    assert_eq!(
        close_interest_count(&host.log()),
        0,
        "adding relay B must not close relay A"
    );

    reconciled_reader.on_kernel_event(&member_event("b-members", RELAY_B, "room-b", &active));
    let snapshot = reader.snapshot();
    assert_eq!(snapshot.groups.len(), 2);

    assert!(close_nip29_joined_groups_session(&host, second));
    assert!(!close_nip29_joined_groups_session(&host, first));
}

#[test]
fn joined_groups_reconcile_shrink_withdraws_and_purges_stale_relay() {
    let host = FakeHost::new();
    let active = "a".repeat(64);
    let (_first, reader) = open_nip29_joined_groups_session_with_reader(
        &host,
        Nip29JoinedGroupsSession::new_for_relays(
            active.clone(),
            vec![RELAY_A.to_string(), RELAY_B.to_string()],
        ),
    )
    .expect("joined-groups opens");
    reader.on_kernel_event(&member_event("a-members", RELAY_A, "room-a", &active));
    reader.on_kernel_event(&member_event("b-members", RELAY_B, "room-b", &active));
    assert_eq!(reader.snapshot().groups.len(), 2);

    let (_second, reconciled_reader) = open_nip29_joined_groups_session_with_reader(
        &host,
        Nip29JoinedGroupsSession::new_for_relays(active, vec![RELAY_B.to_string()]),
    )
    .expect("joined-groups shrinks");

    assert!(Arc::ptr_eq(&reader, &reconciled_reader));
    assert_eq!(host.live_count(), 1);
    assert_eq!(open_count(&host.log()), 2);
    assert_eq!(close_interest_count(&host.log()), 1);

    let snapshot = reconciled_reader.snapshot();
    assert_eq!(snapshot.groups.len(), 1);
    assert_eq!(snapshot.groups[0].group_id, "room-b");
    assert_eq!(snapshot.groups[0].host_relay_url, RELAY_B);
}

#[test]
fn joined_groups_empty_active_pubkey_is_noop() {
    let host = FakeHost::new();
    let handle = open_nip29_joined_groups_session(
        &host,
        Nip29JoinedGroupsSession::new_for_relays(String::new(), vec![RELAY_A.to_string()]),
    );

    assert!(handle.is_none());
    assert_eq!(host.live_count(), 0);
}
