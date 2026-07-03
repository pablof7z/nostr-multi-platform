use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{KernelEvent, ObservedProjection};
use nmp_core::{ObservedProjectionId, ObservedProjectionSink};
use nmp_nip25::decode_reaction_aggregate_snapshot;
use nmp_nip29::GroupId;
use nmp_ownership::ProjectionRegistrationKey;
use nmp_read_session::{
    ReadHost, ReadOutputEncoder, ReadSessionBuild, ReadSessionId, ReadSessionRegistry,
    TeardownAction,
};

use super::*;

const VIEWER: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const OTHER_REACTOR: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const TARGET: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[derive(Clone)]
struct OpenedDemand {
    filter_json: String,
    relay_pin: Option<String>,
    scope: u32,
    replay_limit: usize,
    observer: Arc<dyn ObservedProjectionSink>,
}

#[derive(Default)]
struct FakeHost {
    registry: ReadSessionRegistry,
    demands: Arc<Mutex<Vec<OpenedDemand>>>,
    encoder: Mutex<Option<ReadOutputEncoder>>,
    output_key: Arc<Mutex<Option<String>>>,
    closed_interests: Arc<Mutex<Vec<u64>>>,
    next_interest: Arc<AtomicU64>,
}

impl FakeHost {
    fn demands(&self) -> Vec<OpenedDemand> {
        self.demands.lock().unwrap().clone()
    }

    fn run_encoder(&self) -> Option<nmp_core::TypedProjectionData> {
        self.encoder.lock().unwrap().as_ref().and_then(|e| e())
    }
}

impl ReadHost for FakeHost {
    fn install_read_output(&self, key: ProjectionRegistrationKey, encoder: ReadOutputEncoder) {
        *self.output_key.lock().unwrap() = Some(key.as_str().to_string());
        *self.encoder.lock().unwrap() = Some(encoder);
    }

    fn open_read_interest(&self, decl: ObservedProjection) -> ObservedProjectionId {
        self.demands.lock().unwrap().push(OpenedDemand {
            filter_json: decl.filter_json,
            relay_pin: decl.relay_pin,
            scope: decl.scope,
            replay_limit: decl.replay_limit,
            observer: decl.observer,
        });
        ObservedProjectionId(self.next_interest.fetch_add(1, Ordering::Relaxed) + 1)
    }

    fn teardown_close_interest(&self, id: ObservedProjectionId) -> TeardownAction {
        let closed = Arc::clone(&self.closed_interests);
        Box::new(move || closed.lock().unwrap().push(id.0))
    }

    fn teardown_remove_output(&self, _key: String) -> TeardownAction {
        let output = Arc::clone(&self.output_key);
        Box::new(move || *output.lock().unwrap() = None)
    }

    fn teardown_mark_changed(&self) -> TeardownAction {
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
}

#[test]
fn group_reaction_filter_carries_kinds_5_7_and_h_only() {
    let group = GroupId::new("wss://groups.example.com", "room-a");
    let filter = group_reactions_filter_json(&group);
    let v: serde_json::Value = serde_json::from_str(&filter).unwrap();

    assert_eq!(v["kinds"], serde_json::json!([5, 7]));
    assert_eq!(v["#h"], serde_json::json!(["room-a"]));
    assert!(v.get("relay_pin").is_none());
    assert!(nmp_planner::InterestShape::from_filter_json(&filter).is_some());
}

#[test]
fn open_group_reactions_drives_read_engine_with_group_pin() {
    let host = FakeHost::default();
    let group = GroupId::new("wss://groups.example", "room");

    let (handle, reader) = open_nip25_group_reactions_session_with_reader(
        &host,
        Nip25GroupReactionsSession::new(group, VIEWER.to_string()),
    );

    assert_eq!(handle.key(), GROUP_REACTIONS_KEY);
    assert_eq!(host.registry.live_count(), 1);
    assert_eq!(
        host.output_key.lock().unwrap().as_deref(),
        Some(GROUP_REACTIONS_KEY)
    );
    let demands = host.demands();
    assert_eq!(demands.len(), 1);
    assert_eq!(
        demands[0].relay_pin.as_deref(),
        Some("wss://groups.example")
    );
    assert_eq!(demands[0].scope, 1);
    assert_eq!(demands[0].replay_limit, 80);
    assert!(demands[0].filter_json.contains(r##""#h":["room"]"##));

    demands[0]
        .observer
        .on_kernel_event(&reaction("r1", VIEWER, "+"));
    demands[0]
        .observer
        .on_kernel_event(&reaction("r2", OTHER_REACTOR, "-"));

    let aggregate = reader.aggregate_for(TARGET).expect("target aggregated");
    assert_eq!(aggregate.total, 2);
    assert_eq!(aggregate.mine.len(), 1);
    assert_eq!(aggregate.mine[0].reaction_event_id, "r1");

    let data = host.run_encoder().expect("typed output emits");
    assert_eq!(data.key, GROUP_REACTIONS_KEY);
    let snapshot = decode_reaction_aggregate_snapshot(&data.payload).expect("decodes");
    assert_eq!(snapshot.targets.len(), 1);
    assert_eq!(snapshot.targets[0].target_event_id, TARGET);
    assert_eq!(snapshot.targets[0].total, 2);
}

#[test]
fn group_reactions_replacement_makes_old_handle_stale() {
    let host = FakeHost::default();
    let first = open_nip25_group_reactions_session(
        &host,
        Nip25GroupReactionsSession::new(
            GroupId::new("wss://groups.example", "first"),
            String::new(),
        ),
    );
    assert_eq!(host.registry.live_count(), 1);

    let second = open_nip25_group_reactions_session(
        &host,
        Nip25GroupReactionsSession::new(
            GroupId::new("wss://groups.example", "second"),
            String::new(),
        ),
    );
    assert_eq!(host.registry.live_count(), 1);

    assert!(!close_nip25_group_reactions_session(&host, first));
    assert_eq!(host.registry.live_count(), 1);
    assert!(close_nip25_group_reactions_session(&host, second.clone()));
    assert_eq!(host.registry.live_count(), 0);
    assert!(!close_nip25_group_reactions_session(&host, second));
}

fn reaction(id: &str, author: &str, token: &str) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: 7,
        created_at: 100,
        tags: vec![
            vec!["e".to_string(), TARGET.to_string()],
            vec!["h".to_string(), "room".to_string()],
        ],
        content: token.to_string(),
        relay_provenance: Vec::new(),
    }
}
