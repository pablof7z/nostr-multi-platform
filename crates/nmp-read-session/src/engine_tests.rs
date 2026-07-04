//! Boundary proofs for the engine driver, against a fake host that records the
//! mechanics order. These prove the engine — not any concept — owns install →
//! replay-before-live open → store, and reverse per-demand teardown + tombstone.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{KernelEvent, ObservedProjection};
use nmp_core::{ObservedProjectionId, ObservedProjectionSink};
use nmp_ownership::{DynamicProjectionKey, ProjectionRegistrationKey};
use nmp_planner::InterestShape;

use super::*;
use crate::host::{ReadDemand, ReadDependentDemand, ReadSpec};
use crate::registry::{ReadSessionBuild, ReadSessionId, ReadSessionRegistry, TeardownAction};
use crate::{ReadHost, ReadInterestController, ReadOutputEncoder, ReadReplayPolicy};

#[derive(Default)]
struct NoopSink;
impl ObservedProjectionSink for NoopSink {
    fn on_kernel_event(&self, _event: &KernelEvent) {}
}

struct FakeHost {
    registry: ReadSessionRegistry,
    log: Arc<Mutex<Vec<String>>>,
    observer: Arc<Mutex<Option<Arc<dyn ObservedProjectionSink>>>>,
    next_interest: Arc<AtomicU64>,
    replay_on_controller_open: Arc<Mutex<Option<KernelEvent>>>,
    fail_opens: bool,
}

impl FakeHost {
    fn new() -> Self {
        Self {
            registry: ReadSessionRegistry::default(),
            log: Arc::new(Mutex::new(Vec::new())),
            observer: Arc::new(Mutex::new(None)),
            next_interest: Arc::new(AtomicU64::new(1)),
            replay_on_controller_open: Arc::new(Mutex::new(None)),
            fail_opens: false,
        }
    }
    fn push(&self, entry: impl Into<String>) {
        self.log.lock().unwrap().push(entry.into());
    }
    fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
    fn feed(&self, event: &KernelEvent) {
        let observer = self.observer.lock().unwrap().clone();
        if let Some(observer) = observer.as_ref() {
            observer.on_kernel_event(event);
        }
    }
    fn replay_on_controller_open(&self, event: KernelEvent) {
        *self.replay_on_controller_open.lock().unwrap() = Some(event);
    }
}

impl ReadHost for FakeHost {
    fn install_read_output(&self, key: ProjectionRegistrationKey, _encoder: ReadOutputEncoder) {
        self.push(format!("install:{}", key.as_str()));
    }
    fn open_read_interest(&self, decl: ObservedProjection) -> ObservedProjectionId {
        if self.fail_opens {
            self.push("open_interest_failed");
            return ObservedProjectionId(0);
        }
        *self.observer.lock().unwrap() = Some(Arc::clone(&decl.observer));
        let id = self.next_interest.fetch_add(1, Ordering::Relaxed);
        self.push(format!("open_interest:{}", decl.filter_json));
        self.push(format!(
            "open_indexer_discovery:{}",
            decl.is_indexer_discovery
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
    fn read_interest_controller(&self) -> Option<ReadInterestController> {
        let log = Arc::clone(&self.log);
        let observer_slot = Arc::clone(&self.observer);
        let next_interest = Arc::clone(&self.next_interest);
        let replay_slot = Arc::clone(&self.replay_on_controller_open);
        let open = move |decl: ObservedProjection| {
            *observer_slot.lock().unwrap() = Some(Arc::clone(&decl.observer));
            let id = next_interest.fetch_add(1, Ordering::Relaxed);
            log.lock()
                .unwrap()
                .push(format!("open_interest:{}", decl.filter_json));
            log.lock().unwrap().push(format!(
                "open_indexer_discovery:{}",
                decl.is_indexer_discovery
            ));
            let replay = replay_slot.lock().unwrap().take();
            if let Some(event) = replay {
                decl.observer.on_kernel_event(&event);
            }
            ObservedProjectionId(id)
        };
        let log = Arc::clone(&self.log);
        let close = move |id: ObservedProjectionId| {
            log.lock().unwrap().push(format!("close_interest:{}", id.0));
        };
        Some(ReadInterestController::new(open, close))
    }
}

fn key(value: &str) -> ProjectionRegistrationKey {
    ProjectionRegistrationKey::Dynamic(DynamicProjectionKey::app_owned(value).unwrap())
}

fn demand(filter: &str) -> ReadDemand {
    ReadDemand {
        filter_json: filter.to_string(),
        consumer_id: "consumer".to_string(),
        scope: 0,
        relay_pin: None,
        is_indexer_discovery: false,
        lifecycle: nmp_planner::InterestLifecycle::Tailing,
        replay_limit: 64,
        replay: ReadReplayPolicy::Structural,
    }
}

fn spec(key_value: &str, filters: &[&str]) -> ReadSpec {
    ReadSpec {
        projection_key: key(key_value),
        demands: filters.iter().map(|f| demand(f)).collect(),
        observer: Arc::new(NoopSink),
        output_encoder: Box::new(|| None),
        dependent_demands: Vec::new(),
        keep_open_without_live_demand: false,
    }
}

#[test]
fn open_installs_output_before_opening_live_demand() {
    let host = FakeHost::new();
    let handle = open_read(
        &host,
        spec("app.test.read", &[r##"{"kinds":[1],"#e":["aa"]}"##]),
    );
    let log = host.log();
    let install = log.iter().position(|e| e.starts_with("install:")).unwrap();
    let open = log
        .iter()
        .position(|e| e.starts_with("open_interest:"))
        .unwrap();
    assert!(
        install < open,
        "typed output installed before live demand opens (no first-tick gap): {log:?}"
    );
    assert_eq!(host.registry.live_count(), 1, "one live read");
    assert_ne!(handle.session_id, ReadSessionId(0));
}

#[test]
fn composed_demands_share_one_reducer_and_each_is_withdrawn() {
    let host = FakeHost::new();
    // Two demands (à la NIP-10 kind:1 + NIP-22 kind:1111) → one read.
    let handle = open_read(
        &host,
        spec(
            "app.test.read",
            &[
                r##"{"kinds":[1],"#e":["aa"]}"##,
                r##"{"kinds":[1111],"#E":["aa"]}"##,
            ],
        ),
    );
    let opened = host
        .log()
        .iter()
        .filter(|e| e.starts_with("open_interest:"))
        .count();
    assert_eq!(opened, 2, "both conventions open live demand");

    assert!(close_read(&host, &handle));
    let log = host.log();
    let closes: Vec<_> = log
        .iter()
        .filter(|e| e.starts_with("close_interest:"))
        .cloned()
        .collect();
    assert_eq!(
        closes.len(),
        2,
        "every demand is withdrawn on close (exact)"
    );
    // Reverse teardown: both interests withdrawn, then output tombstoned, then tick.
    let remove = log
        .iter()
        .position(|e| e.starts_with("remove_output:"))
        .unwrap();
    let last_close = log
        .iter()
        .rposition(|e| e.starts_with("close_interest:"))
        .unwrap();
    let mark = log.iter().position(|e| e == "mark_changed").unwrap();
    assert!(
        last_close < remove,
        "interests withdrawn before output tombstone"
    );
    assert!(remove < mark, "output tombstoned before the change flag");
    assert_eq!(host.registry.live_count(), 0, "no leak after close");
}

#[test]
fn close_is_idempotent_and_rejects_a_forged_key() {
    let host = FakeHost::new();
    let handle = open_read(
        &host,
        spec("app.test.read", &[r##"{"kinds":[1],"#e":["aa"]}"##]),
    );

    let forged = ReadHandle {
        projection_key: "app.test.other".to_string(),
        session_id: handle.session_id,
    };
    assert!(
        !close_read(&host, &forged),
        "a mismatched key never closes the read"
    );

    assert!(close_read(&host, &handle), "the real handle closes it");
    assert!(!close_read(&host, &handle), "second close is a no-op");
}

#[test]
fn demand_indexer_discovery_flag_reaches_observed_interest() {
    let host = FakeHost::new();
    let mut read = demand(r##"{"kinds":[3],"authors":["aa"]}"##);
    read.is_indexer_discovery = true;

    let _handle = open_read(
        &host,
        ReadSpec {
            projection_key: key("app.test.discovery"),
            demands: vec![read],
            observer: Arc::new(NoopSink),
            output_encoder: Box::new(|| None),
            dependent_demands: Vec::new(),
            keep_open_without_live_demand: false,
        },
    );

    let log = host.log();
    assert!(
        log.iter()
            .any(|entry| entry == "open_indexer_discovery:true"),
        "read demand opt-in must reach the opened observed projection: {log:?}"
    );
}

#[test]
fn a_read_that_keeps_nothing_live_is_not_tracked() {
    let mut host = FakeHost::new();
    host.fail_opens = true;
    let handle = open_read(
        &host,
        spec("app.test.read", &[r##"{"kinds":[1],"#e":["aa"]}"##]),
    );
    assert_eq!(handle.session_id, ReadSessionId(0), "sentinel handle");
    assert_eq!(host.registry.live_count(), 0, "no dead read tracked");
    // The output we installed was tombstoned immediately (no leaked sidecar).
    assert!(host.log().iter().any(|e| e.starts_with("remove_output:")));
}

#[derive(Default)]
struct DerivedDemandSink {
    desired: Arc<Mutex<Option<ReadDependentDemand>>>,
}

impl ObservedProjectionSink for DerivedDemandSink {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if event.kind == 1 {
            let mut tags = std::collections::BTreeMap::new();
            tags.insert(
                "e".to_string(),
                std::collections::BTreeSet::from([event.id.clone()]),
            );
            *self.desired.lock().unwrap() = Some(ReadDependentDemand {
                shape: InterestShape {
                    kinds: std::collections::BTreeSet::from([5]),
                    tags,
                    ..Default::default()
                },
                scope: 1,
                is_indexer_discovery: true,
                replay_limit: 32,
            });
        } else if event.kind == 5 {
            *self.desired.lock().unwrap() = None;
        }
    }
}

#[test]
fn dependent_demand_opens_from_reducer_state_and_closes_with_the_read() {
    let host = FakeHost::new();
    let desired = Arc::new(Mutex::new(None));
    let provider = {
        let desired = Arc::clone(&desired);
        Arc::new(move || desired.lock().unwrap().clone())
    };
    let handle = open_read(
        &host,
        ReadSpec {
            projection_key: key("app.test.read"),
            demands: vec![demand(r##"{"kinds":[1],"#e":["root"]}"##)],
            observer: Arc::new(DerivedDemandSink {
                desired: Arc::clone(&desired),
            }),
            output_encoder: Box::new(|| None),
            dependent_demands: vec![provider],
            keep_open_without_live_demand: false,
        },
    );

    let admitted_id = "aa".repeat(32);
    host.feed(&KernelEvent {
        id: admitted_id.clone(),
        author: "bb".repeat(32),
        kind: 1,
        created_at: 1,
        tags: vec![vec!["e".to_string(), "root".to_string()]],
        content: String::new(),
        relay_provenance: Vec::new(),
    });

    let log = host.log();
    let opens = log
        .iter()
        .filter(|entry| entry.starts_with("open_interest:"))
        .collect::<Vec<_>>();
    assert_eq!(opens.len(), 2, "primary + derived demand were opened");
    assert!(
        opens[1].contains(r#""kinds":[5]"#)
            && opens[1].contains(&format!(r##""#e":["{admitted_id}"]"##)),
        "derived demand routes kind:5 deletes by admitted event id: {log:?}"
    );
    let indexer_flags = log
        .iter()
        .filter(|entry| entry.starts_with("open_indexer_discovery:"))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        indexer_flags,
        vec![
            "open_indexer_discovery:false",
            "open_indexer_discovery:true"
        ],
        "primary and derived read demands preserve separate routing flags"
    );

    assert!(close_read(&host, &handle));
    let closes = host
        .log()
        .iter()
        .filter(|entry| entry.starts_with("close_interest:"))
        .count();
    assert_eq!(closes, 2, "derived and primary interests are withdrawn");
}

#[test]
fn dependent_demand_reconciles_replay_reentry_during_derived_open() {
    let host = FakeHost::new();
    let desired = Arc::new(Mutex::new(None));
    let provider = {
        let desired = Arc::clone(&desired);
        Arc::new(move || desired.lock().unwrap().clone())
    };
    let handle = open_read(
        &host,
        ReadSpec {
            projection_key: key("app.test.read"),
            demands: vec![demand(r##"{"kinds":[1],"#e":["root"]}"##)],
            observer: Arc::new(DerivedDemandSink {
                desired: Arc::clone(&desired),
            }),
            output_encoder: Box::new(|| None),
            dependent_demands: vec![provider],
            keep_open_without_live_demand: false,
        },
    );

    let admitted_id = "aa".repeat(32);
    host.replay_on_controller_open(KernelEvent {
        id: "cc".repeat(32),
        author: "bb".repeat(32),
        kind: 5,
        created_at: 2,
        tags: vec![vec!["e".to_string(), admitted_id.clone()]],
        content: String::new(),
        relay_provenance: Vec::new(),
    });
    host.feed(&KernelEvent {
        id: admitted_id,
        author: "bb".repeat(32),
        kind: 1,
        created_at: 1,
        tags: vec![vec!["e".to_string(), "root".to_string()]],
        content: String::new(),
        relay_provenance: Vec::new(),
    });

    let log = host.log();
    assert_eq!(
        log.iter()
            .filter(|entry| entry.starts_with("open_interest:"))
            .count(),
        2,
        "primary plus derived demand opened"
    );
    assert_eq!(
        log.iter()
            .filter(|entry| entry.starts_with("close_interest:"))
            .count(),
        1,
        "replayed delete dirties the reconciler and withdraws derived demand"
    );

    assert!(close_read(&host, &handle));
    assert_eq!(
        host.log()
            .iter()
            .filter(|entry| entry.starts_with("close_interest:"))
            .count(),
        2,
        "only the primary demand remains for session close"
    );
}

#[test]
fn seed_only_read_can_stay_live_until_closed_by_key() {
    let host = FakeHost::new();
    let handle = open_read(
        &host,
        ReadSpec {
            projection_key: key("app.test.seed"),
            demands: Vec::new(),
            observer: Arc::new(NoopSink),
            output_encoder: Box::new(|| None),
            dependent_demands: Vec::new(),
            keep_open_without_live_demand: true,
        },
    );

    assert_ne!(handle.session_id, ReadSessionId(0));
    assert_eq!(host.registry.live_count(), 1);
    assert!(
        host.close_read_session_by_projection_key("app.test.seed"),
        "legacy key-addressed close uses the shared engine registry"
    );
    assert_eq!(host.registry.live_count(), 0);
    let log = host.log();
    assert!(log.iter().any(|e| e == "remove_output:app.test.seed"));
    assert!(log.iter().any(|e| e == "mark_changed"));
}
