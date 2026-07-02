//! Boundary proofs for the engine driver, against a fake host that records the
//! mechanics order. These prove the engine — not any concept — owns install →
//! replay-before-live open → store, and reverse per-demand teardown + tombstone.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{KernelEvent, ObservedProjection};
use nmp_core::{ObservedProjectionId, ObservedProjectionSink};
use nmp_ownership::{DynamicProjectionKey, ProjectionRegistrationKey};

use super::*;
use crate::host::{ReadDemand, ReadSpec};
use crate::registry::{ReadSessionBuild, ReadSessionId, ReadSessionRegistry, TeardownAction};
use crate::{ReadHost, ReadOutputEncoder};

#[derive(Default)]
struct NoopSink;
impl ObservedProjectionSink for NoopSink {
    fn on_kernel_event(&self, _event: &KernelEvent) {}
}

struct FakeHost {
    registry: ReadSessionRegistry,
    log: Arc<Mutex<Vec<String>>>,
    next_interest: AtomicU64,
    fail_opens: bool,
}

impl FakeHost {
    fn new() -> Self {
        Self {
            registry: ReadSessionRegistry::default(),
            log: Arc::new(Mutex::new(Vec::new())),
            next_interest: AtomicU64::new(1),
            fail_opens: false,
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
        if self.fail_opens {
            self.push("open_interest_failed");
            return ObservedProjectionId(0);
        }
        let id = self.next_interest.fetch_add(1, Ordering::Relaxed);
        self.push(format!("open_interest:{}", decl.filter_json));
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
        replay_limit: 64,
    }
}

fn spec(key_value: &str, filters: &[&str]) -> ReadSpec {
    ReadSpec {
        projection_key: key(key_value),
        demands: filters.iter().map(|f| demand(f)).collect(),
        observer: Arc::new(NoopSink),
        output_encoder: Box::new(|| None),
    }
}

#[test]
fn open_installs_output_before_opening_live_demand() {
    let host = FakeHost::new();
    let handle = open_read(&host, spec("app.test.read", &[r##"{"kinds":[1],"#e":["aa"]}"##]));
    let log = host.log();
    let install = log.iter().position(|e| e.starts_with("install:")).unwrap();
    let open = log.iter().position(|e| e.starts_with("open_interest:")).unwrap();
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
            &[r##"{"kinds":[1],"#e":["aa"]}"##, r##"{"kinds":[1111],"#E":["aa"]}"##],
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
    assert_eq!(closes.len(), 2, "every demand is withdrawn on close (exact)");
    // Reverse teardown: both interests withdrawn, then output tombstoned, then tick.
    let remove = log.iter().position(|e| e.starts_with("remove_output:")).unwrap();
    let last_close = log.iter().rposition(|e| e.starts_with("close_interest:")).unwrap();
    let mark = log.iter().position(|e| e == "mark_changed").unwrap();
    assert!(last_close < remove, "interests withdrawn before output tombstone");
    assert!(remove < mark, "output tombstoned before the change flag");
    assert_eq!(host.registry.live_count(), 0, "no leak after close");
}

#[test]
fn close_is_idempotent_and_rejects_a_forged_key() {
    let host = FakeHost::new();
    let handle = open_read(&host, spec("app.test.read", &[r##"{"kinds":[1],"#e":["aa"]}"##]));

    let forged = ReadHandle {
        projection_key: "app.test.other".to_string(),
        session_id: handle.session_id,
    };
    assert!(!close_read(&host, &forged), "a mismatched key never closes the read");

    assert!(close_read(&host, &handle), "the real handle closes it");
    assert!(!close_read(&host, &handle), "second close is a no-op");
}

#[test]
fn a_read_that_keeps_nothing_live_is_not_tracked() {
    let mut host = FakeHost::new();
    host.fail_opens = true;
    let handle = open_read(&host, spec("app.test.read", &[r##"{"kinds":[1],"#e":["aa"]}"##]));
    assert_eq!(handle.session_id, ReadSessionId(0), "sentinel handle");
    assert_eq!(host.registry.live_count(), 0, "no dead read tracked");
    // The output we installed was tombstoned immediately (no leaked sidecar).
    assert!(host.log().iter().any(|e| e.starts_with("remove_output:")));
}
