//! Split out of `keyed_collection_tests.rs` (file-size soft cap, AGENTS.md):
//! the "flavor 2" boundary proof — each key mounts a FULL, independent
//! [`crate::open_read`] read-session — its own reducer, its own typed
//! output, its own [`crate::ReadHandle`] — via a real [`crate::ReadHost`].
//! This is shape (b)'s defining difference from `demand_set`'s shape (a): N
//! members, N reducers/outputs, not one shared across the set.

use super::*;
use nmp_core::substrate::{KernelEvent, ObservedProjection};
use nmp_core::{ObservedProjectionId, ObservedProjectionSink};
use nmp_ownership::{DynamicProjectionKey, ProjectionRegistrationKey};

use crate::host::{DemandSetReconciler, ReadDemand};
use crate::registry::{DemandSetMembers, ReadSessionBuild, ReadSessionId, ReadSessionRegistry};
use crate::{
    close_read, open_read, ReadHandle, ReadHost, ReadOutputEncoder, ReadReplayPolicy, ReadSpec,
};

#[derive(Default)]
struct RecordingSink {
    seen: Mutex<Vec<String>>,
}

impl ObservedProjectionSink for RecordingSink {
    fn on_kernel_event(&self, event: &KernelEvent) {
        self.seen.lock().unwrap().push(event.id.clone());
    }
}

struct FakeHost {
    registry: ReadSessionRegistry,
    next_interest: AtomicU64,
}

impl FakeHost {
    fn new() -> Self {
        Self {
            registry: ReadSessionRegistry::default(),
            next_interest: AtomicU64::new(1),
        }
    }
}

impl ReadHost for FakeHost {
    fn install_read_output(&self, _key: ProjectionRegistrationKey, _encoder: ReadOutputEncoder) {}
    fn open_read_interest(&self, _decl: ObservedProjection) -> ObservedProjectionId {
        ObservedProjectionId(self.next_interest.fetch_add(1, Ordering::Relaxed))
    }
    fn teardown_close_interest(&self, _id: ObservedProjectionId) -> crate::TeardownAction {
        Box::new(|| {})
    }
    fn teardown_remove_output(&self, _key: String) -> crate::TeardownAction {
        Box::new(|| {})
    }
    fn teardown_mark_changed(&self) -> crate::TeardownAction {
        Box::new(|| {})
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

fn projection_key(group_id: &str) -> ProjectionRegistrationKey {
    ProjectionRegistrationKey::Dynamic(
        DynamicProjectionKey::app_owned(format!("group-feed.{group_id}")).unwrap(),
    )
}

fn group_demand(group_id: &str) -> ReadDemand {
    ReadDemand {
        filter_json: format!(r##"{{"kinds":[9],"#h":["{group_id}"]}}"##),
        consumer_id: format!("group-feed::{group_id}"),
        scope: 1,
        relay_pin: None,
        is_indexer_discovery: false,
        lifecycle: nmp_planner::InterestLifecycle::Tailing,
        replay_limit: 64,
        replay: ReadReplayPolicy::Structural,
    }
}

#[test]
fn each_key_mounts_its_own_independent_read_session() {
    let host = Arc::new(FakeHost::new());
    let host_for_open = Arc::clone(&host);
    let collection: KeyedReadCollection<String, String> = KeyedReadCollection::new(
        "group-feeds",
        |group_id: &String| MemberKey::new(group_id.clone()),
        move |resource_key, group_id: String| {
            let host_for_close = Arc::clone(&host_for_open);
            // Each key builds its OWN reducer + output — shape (b)'s
            // defining property, unlike demand_set's one-reducer-for-all.
            let spec = ReadSpec {
                projection_key: projection_key(&group_id),
                demands: vec![group_demand(&group_id)],
                observer: Arc::new(RecordingSink::default()),
                output_encoder: Box::new(|| None),
                dependent_demands: Vec::new(),
                keep_open_without_live_demand: false,
            };
            let handle: ReadHandle = open_read(host_for_open.as_ref(), spec);
            let _ = resource_key;
            Box::new(move || {
                let _ = close_read(host_for_close.as_ref(), &handle);
            }) as crate::registry::TeardownAction
        },
    );

    let mut desired = BTreeMap::new();
    desired.insert("group-1".to_string(), "group-1".to_string());
    desired.insert("group-2".to_string(), "group-2".to_string());
    collection.reconcile(desired);

    assert_eq!(collection.live_count(), 2);
    assert_eq!(
        host.registry.live_count(),
        2,
        "two independent read-sessions, one per key"
    );

    collection.close();
    assert_eq!(collection.live_count(), 0);
    assert_eq!(host.registry.live_count(), 0);
}
