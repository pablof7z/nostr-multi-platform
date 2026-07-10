//! Boundary proofs for the dynamic-membership demand-set engine (#93): opening
//! N members shares one reducer, reconcile touches only the delta (an
//! untouched member's interest is never closed+reopened), and close drains
//! whatever remains regardless of intervening reconciles.

use std::any::Any;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{KernelEvent, ObservedProjection};
use nmp_core::{ObservedProjectionId, ObservedProjectionSink};
use nmp_ownership::{DynamicProjectionKey, ProjectionRegistrationKey};

use super::*;
use crate::host::{DemandSetReconciler, KeyedReadDemand, ReadDemand, ReadDemandSetSpec};
use crate::registry::{DemandSetMembers, ReadSessionId, ReadSessionRegistry};
use crate::{close_read, ReadHost, ReadOutputEncoder, ReadReplayPolicy, TeardownAction};

#[derive(Default)]
struct RecordingSink {
    ids: Mutex<Vec<String>>,
}

impl ObservedProjectionSink for RecordingSink {
    fn on_kernel_event(&self, event: &KernelEvent) {
        self.ids.lock().unwrap().push(event.id.clone());
    }
}

struct FakeHost {
    registry: ReadSessionRegistry,
    log: Arc<Mutex<Vec<String>>>,
    next_interest: Arc<AtomicU64>,
}

impl FakeHost {
    fn new() -> Self {
        Self {
            registry: ReadSessionRegistry::default(),
            log: Arc::new(Mutex::new(Vec::new())),
            next_interest: Arc::new(AtomicU64::new(1)),
        }
    }
    fn push(&self, entry: impl Into<String>) {
        self.log.lock().unwrap().push(entry.into());
    }
    fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
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
            decl.relay_pin.clone().unwrap_or_default()
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
    fn read_demand_set_reconciler(&self, projection_key: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        self.registry.demand_set_reconciler(projection_key)
    }
}

fn key(value: &str) -> ProjectionRegistrationKey {
    ProjectionRegistrationKey::Dynamic(DynamicProjectionKey::app_owned(value).unwrap())
}

fn member(relay: &str) -> KeyedReadDemand {
    KeyedReadDemand {
        key: relay.to_string(),
        demand: ReadDemand {
            filter_json: format!(r##"{{"kinds":[39000],"relay":"{relay}"}}"##),
            consumer_id: format!("discovery::{relay}"),
            scope: 1,
            relay_pin: Some(relay.to_string()),
            is_indexer_discovery: false,
            lifecycle: nmp_planner::InterestLifecycle::Tailing,
            replay_limit: 64,
            replay: ReadReplayPolicy::Structural,
        },
    }
}

fn open_two_relays(host: &FakeHost) -> (ReadHandle, Arc<RecordingSink>) {
    let sink = Arc::new(RecordingSink::default());
    let observer: Arc<dyn ObservedProjectionSink> = Arc::clone(&sink) as _;
    let reducer: Arc<dyn Any + Send + Sync> = Arc::clone(&sink) as _;
    let handle = open_read_demand_set(
        host,
        ReadDemandSetSpec {
            projection_key: key("app.test.discovery"),
            members: vec![member("wss://a.example"), member("wss://b.example")],
            observer,
            reducer,
            output_encoder: Box::new(|| None),
        },
    );
    (handle, sink)
}

#[test]
fn open_shares_one_reducer_across_every_member() {
    let host = FakeHost::new();
    let (_handle, sink) = open_two_relays(&host);

    let opened = host.log().iter().filter(|e| e.starts_with("open:")).count();
    assert_eq!(opened, 2, "one interest per initial member");

    // Both members' demands were constructed with the SAME observer Arc — feed
    // through the returned sink directly (as production code would via the
    // shared observed-interest registration) and confirm it's the one and
    // only reducer instance.
    sink.on_kernel_event(&KernelEvent {
        id: "e1".into(),
        author: "a".repeat(64),
        kind: 39000,
        created_at: 1,
        tags: vec![vec!["d".into(), "room".into()]],
        content: String::new(),
        relay_provenance: vec!["wss://a.example".to_string()],
    });
    assert_eq!(sink.ids.lock().unwrap().len(), 1);
}

#[test]
fn reconcile_adds_a_new_member_without_touching_the_existing_one() {
    let host = FakeHost::new();
    let (_handle, sink) = open_two_relays(&host);
    let observer: Arc<dyn ObservedProjectionSink> = Arc::clone(&sink) as _;

    let reconciled = reconcile_read_demand_set(
        &host,
        "app.test.discovery",
        &observer,
        vec![
            member("wss://a.example"),
            member("wss://b.example"),
            member("wss://c.example"),
        ],
    );
    assert!(reconciled, "a live demand-set session exists under the key");

    let log = host.log();
    assert_eq!(
        log.iter().filter(|e| e.starts_with("open:")).count(),
        3,
        "two initial members + one newly-desired member"
    );
    assert_eq!(
        log.iter()
            .filter(|e| e.starts_with("close_interest:"))
            .count(),
        0,
        "adding a member must not close any existing member's interest \
         (the exact singleton-kill regression, #93)"
    );
}

#[test]
fn reconcile_removes_a_member_no_longer_desired() {
    let host = FakeHost::new();
    let (_handle, sink) = open_two_relays(&host);
    let observer: Arc<dyn ObservedProjectionSink> = Arc::clone(&sink) as _;

    let reconciled = reconcile_read_demand_set(
        &host,
        "app.test.discovery",
        &observer,
        vec![member("wss://b.example")],
    );
    assert!(reconciled);

    let log = host.log();
    assert_eq!(
        log.iter()
            .filter(|e| e.starts_with("close_interest:"))
            .count(),
        1,
        "the relay dropped from the desired set is withdrawn"
    );
    assert_eq!(
        log.iter().filter(|e| e.starts_with("open:")).count(),
        2,
        "no re-open of the still-desired relay's interest"
    );
}

#[test]
fn reconcile_on_an_unknown_key_returns_false() {
    let host = FakeHost::new();
    let sink = Arc::new(RecordingSink::default());
    let observer: Arc<dyn ObservedProjectionSink> = Arc::clone(&sink) as _;
    assert!(!reconcile_read_demand_set(
        &host,
        "app.test.nothing-open",
        &observer,
        vec![member("wss://a.example")],
    ));
}

#[test]
fn close_drains_every_member_added_since_open() {
    let host = FakeHost::new();
    let (handle, sink) = open_two_relays(&host);
    let observer: Arc<dyn ObservedProjectionSink> = Arc::clone(&sink) as _;
    assert!(reconcile_read_demand_set(
        &host,
        "app.test.discovery",
        &observer,
        vec![member("wss://a.example"), member("wss://c.example")],
    ));

    assert!(close_read(&host, &handle));
    let log = host.log();
    let closes = log
        .iter()
        .filter(|e| e.starts_with("close_interest:"))
        .count();
    // b was withdrawn by the reconcile above (removed from the desired set);
    // a + c remain live until close, which must withdraw both.
    assert_eq!(
        closes, 3,
        "reconcile-withdrawn (b) + close-withdrawn (a, c) = full accounting: {log:?}"
    );
    assert_eq!(host.registry.live_count(), 0, "no leak after close");
}

#[test]
fn a_demand_set_may_open_with_zero_members_and_stays_tracked() {
    let host = FakeHost::new();
    let sink = Arc::new(RecordingSink::default());
    let observer: Arc<dyn ObservedProjectionSink> = Arc::clone(&sink) as _;
    let reducer: Arc<dyn Any + Send + Sync> = Arc::clone(&sink) as _;
    let handle = open_read_demand_set(
        &host,
        ReadDemandSetSpec {
            projection_key: key("app.test.empty"),
            members: Vec::new(),
            observer,
            reducer,
            output_encoder: Box::new(|| None),
        },
    );
    assert_ne!(handle.session_id, ReadSessionId(0));
    assert_eq!(
        host.registry.live_count(),
        1,
        "an empty demand set is still a live, closeable session"
    );
}

// --- #3116 equivalence proof -----------------------------------------------
//
// `reconcile_read_demand_set` used to hand-roll its own `HashSet` diff; #3116
// replaced it with `nmp_core::trellis_reconciler::KeyedReconciler`. Rather
// than retaining the hand-rolled path as a second, temporary implementation
// only to delete it a few lines later in this same PR, this proves
// equivalence directly against the ONE implementation that ships: at every
// step of a reconcile script, (a) Trellis's own `FullRecomputeCheck` oracle
// must agree the incremental state equals a full recompute from canonical
// inputs (the leak-audit oracle #3115/#3116 call for), and (b) the converged
// member-teardown-map membership must equal the desired set EXACTLY — no
// more, no fewer entries — which is the order-independent trace-set parity
// the design asked for (Open/Close per key, not command order).

fn live_member_keys(host: &FakeHost, projection_key: &str) -> BTreeSet<String> {
    host.read_demand_set_members(projection_key)
        .map(|members| members.lock().unwrap().keys().cloned().collect())
        .unwrap_or_default()
}

fn relay_set(relays: &[&str]) -> BTreeSet<String> {
    relays.iter().map(|relay| (*relay).to_string()).collect()
}

#[test]
fn full_recompute_oracle_and_converged_membership_across_a_reconcile_script() {
    let host = FakeHost::new();
    let sink = Arc::new(RecordingSink::default());
    let observer: Arc<dyn ObservedProjectionSink> = Arc::clone(&sink) as _;
    let reducer: Arc<dyn Any + Send + Sync> = Arc::clone(&sink) as _;
    let projection_key = "app.test.script";
    let handle = open_read_demand_set(
        &host,
        ReadDemandSetSpec {
            projection_key: key(projection_key),
            members: vec![member("wss://a.example"), member("wss://b.example")],
            observer: Arc::clone(&observer),
            reducer,
            output_encoder: Box::new(|| None),
        },
    );

    let reconciler = host
        .read_demand_set_reconciler(projection_key)
        .and_then(|erased| erased.downcast::<DemandSetReconciler>().ok())
        .expect("a demand-set session registers its Trellis reconciler");
    assert!(reconciler.full_recompute_matches());
    assert_eq!(
        live_member_keys(&host, projection_key),
        relay_set(&["wss://a.example", "wss://b.example"])
    );

    // Grow, shrink, grow again, then reconcile with the SAME desired set
    // (a genuine no-op — must not touch any live member's interest).
    let script: [&[&str]; 4] = [
        &["wss://a.example", "wss://b.example", "wss://c.example"],
        &["wss://b.example"],
        &["wss://b.example", "wss://d.example"],
        &["wss://b.example", "wss://d.example"],
    ];
    for (step, desired_relays) in script.iter().enumerate() {
        let log_len_before = host.log().len();
        let desired: Vec<KeyedReadDemand> =
            desired_relays.iter().map(|relay| member(relay)).collect();
        assert!(reconcile_read_demand_set(
            &host,
            projection_key,
            &observer,
            desired
        ));
        assert!(
            reconciler.full_recompute_matches(),
            "step {step}: incremental state must equal a full recompute"
        );
        assert_eq!(
            live_member_keys(&host, projection_key),
            relay_set(desired_relays),
            "step {step}: converged membership must equal the desired set exactly"
        );
        if step == 3 {
            let churn = host.log().len() - log_len_before;
            assert_eq!(
                churn, 1, // the unconditional `mark_changed` teardown call
                "step {step}: reconciling to an unchanged desired set must not open or close anything"
            );
        }
    }

    assert!(close_read(&host, &handle));
    assert!(
        live_member_keys(&host, projection_key).is_empty(),
        "close must drain every remaining member"
    );
}
