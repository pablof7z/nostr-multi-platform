//! Concept-side proofs for `open_zaps`: the door compiles the demand, admits
//! validated zaps, aggregates raw per-sender totals, emits typed output, and
//! drives the ONE engine — with no lifecycle code and no viewer-identity
//! dependency of its own (a fake host records the engine calls).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{KernelEvent, ObservedProjection};
use nmp_core::{ObservedProjectionId, ObservedProjectionSink};
use nmp_ownership::ProjectionRegistrationKey;
use nmp_read_session::{
    ReadHost, ReadOutputEncoder, ReadSessionBuild, ReadSessionId, ReadSessionRegistry,
    TeardownAction,
};

use super::generated::nmp::zaps::root_as_zap_summary_snapshot;
use super::*;
use crate::target::ZapTarget;

const TARGET: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const OTHER: &str = "2222222222222222222222222222222222222222222222222222222222222222";

fn target() -> ZapTarget {
    ZapTarget::event(TARGET).expect("valid hex64 id")
}

fn receipt(id: &str, target_id: &str, sender: Option<&str>, amount: u64) -> KernelEvent {
    let sender_json = sender
        .map(|s| format!("\"pubkey\":\"{s}\",\"tags\":[[\"amount\",\"{amount}\"]]"))
        .unwrap_or_else(|| format!("\"tags\":[[\"amount\",\"{amount}\"]]"));
    KernelEvent {
        id: id.to_string(),
        author: "ln_provider".to_string(),
        kind: 9735,
        created_at: 1,
        tags: vec![
            vec!["p".to_string(), "recipient".to_string()],
            vec!["e".to_string(), target_id.to_string()],
            vec!["description".to_string(), format!("{{{sender_json}}}")],
        ],
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

// ── The concept reducer (no engine involved) ────────────────────────────────

#[test]
fn reducer_aggregates_total_and_count_across_distinct_senders() {
    let reducer = ZapSummaryProjection::new(target());
    reducer.on_kernel_event(&receipt("Z1", TARGET, Some("alice"), 10_000));
    reducer.on_kernel_event(&receipt("Z2", TARGET, Some("bob"), 20_000));
    reducer.on_kernel_event(&receipt("Z3", TARGET, Some("alice"), 5_000));

    let snapshot = reducer.snapshot();
    assert_eq!(snapshot.target_id, TARGET);
    assert_eq!(snapshot.total_msats, 35_000);
    assert_eq!(snapshot.zap_count, 3);
    let alice = snapshot
        .zappers
        .iter()
        .find(|z| z.pubkey.as_deref() == Some("alice"))
        .expect("alice aggregated");
    assert_eq!(alice.total_msats, 15_000);
    assert_eq!(alice.zap_count, 2);
    let bob = snapshot
        .zappers
        .iter()
        .find(|z| z.pubkey.as_deref() == Some("bob"))
        .expect("bob aggregated");
    assert_eq!(bob.total_msats, 20_000);
    assert_eq!(bob.zap_count, 1);
}

#[test]
fn reducer_ignores_receipts_for_a_different_target() {
    let reducer = ZapSummaryProjection::new(target());
    reducer.on_kernel_event(&receipt("Z1", OTHER, Some("alice"), 10_000));
    let snapshot = reducer.snapshot();
    assert_eq!(snapshot.zap_count, 0);
    assert_eq!(snapshot.total_msats, 0);
}

#[test]
fn duplicate_receipt_delivery_does_not_double_count() {
    let reducer = ZapSummaryProjection::new(target());
    let event = receipt("Z1", TARGET, Some("alice"), 10_000);
    reducer.on_kernel_event(&event);
    reducer.on_kernel_event(&event);
    let snapshot = reducer.snapshot();
    assert_eq!(snapshot.zap_count, 1);
    assert_eq!(snapshot.total_msats, 10_000);
}

#[test]
fn anonymous_receipts_aggregate_into_one_bucket() {
    let reducer = ZapSummaryProjection::new(target());
    reducer.on_kernel_event(&receipt("Z1", TARGET, None, 10_000));
    reducer.on_kernel_event(&receipt("Z2", TARGET, None, 5_000));
    let snapshot = reducer.snapshot();
    assert_eq!(snapshot.zap_count, 2);
    assert_eq!(snapshot.zappers.len(), 1, "one anonymous bucket");
    assert_eq!(snapshot.zappers[0].pubkey, None);
    assert_eq!(snapshot.zappers[0].total_msats, 15_000);
    assert_eq!(snapshot.zappers[0].zap_count, 2);
}

#[test]
fn a_caller_derives_viewer_zapped_from_the_raw_zappers_list() {
    // This concept exposes no viewer-relative field (per #2777/#2758 review):
    // a shell membership-checks its own pubkey against `zappers` itself.
    let reducer = ZapSummaryProjection::new(target());
    reducer.on_kernel_event(&receipt("Z1", TARGET, Some("alice"), 10_000));
    reducer.on_kernel_event(&receipt("Z2", TARGET, Some("bob"), 20_000));

    let snapshot = reducer.snapshot();
    let viewer_zapped = snapshot.zappers.iter().any(|z| z.pubkey.as_deref() == Some("alice"));
    assert!(viewer_zapped);
    let viewer_total = snapshot
        .zappers
        .iter()
        .find(|z| z.pubkey.as_deref() == Some("alice"))
        .map(|z| z.total_msats)
        .unwrap_or(0);
    assert_eq!(viewer_total, 10_000);
}

#[test]
fn typed_output_round_trips() {
    let snapshot = ZapSummarySnapshot {
        target_id: TARGET.to_string(),
        total_msats: 15_000,
        zap_count: 1,
        zappers: vec![ZapperTotal {
            pubkey: Some("alice".to_string()),
            total_msats: 15_000,
            zap_count: 1,
        }],
    };
    let bytes = encode_zap_summary_snapshot(&snapshot);
    let decoded = root_as_zap_summary_snapshot(&bytes).unwrap();
    assert_eq!(decoded.schema_version(), ZAP_SUMMARY_SCHEMA_VERSION);
    assert_eq!(decoded.target_id(), Some(TARGET));
    assert_eq!(decoded.total_msats(), 15_000);
    assert_eq!(decoded.zap_count(), 1);
    let zappers = decoded.zappers().unwrap();
    assert_eq!(zappers.len(), 1);
    assert_eq!(zappers.get(0).pubkey(), Some("alice"));
    assert_eq!(zappers.get(0).total_msats(), 15_000);
}

// ── The door drives the engine end-to-end (fake host) ───────────────────────

#[derive(Default)]
struct FakeHost {
    registry: ReadSessionRegistry,
    observer: Mutex<Option<Arc<dyn ObservedProjectionSink>>>,
    encoder: Mutex<Option<ReadOutputEncoder>>,
    output_key: Arc<Mutex<Option<String>>>,
    opened_filters: Mutex<Vec<String>>,
    closed_interests: Arc<Mutex<Vec<u64>>>,
    next_interest: AtomicU64,
}

impl FakeHost {
    fn run_encoder(&self) -> Option<nmp_core::TypedProjectionData> {
        self.encoder.lock().unwrap().as_ref().and_then(|e| e())
    }
    fn feed(&self, event: &KernelEvent) {
        if let Some(obs) = self.observer.lock().unwrap().as_ref() {
            obs.on_kernel_event(event);
        }
    }
}

impl ReadHost for FakeHost {
    fn install_read_output(&self, key: ProjectionRegistrationKey, encoder: ReadOutputEncoder) {
        *self.output_key.lock().unwrap() = Some(key.as_str().to_string());
        *self.encoder.lock().unwrap() = Some(encoder);
    }
    fn open_read_interest(&self, decl: ObservedProjection) -> ObservedProjectionId {
        *self.observer.lock().unwrap() = Some(Arc::clone(&decl.observer));
        self.opened_filters.lock().unwrap().push(decl.filter_json);
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
}

#[test]
fn open_zaps_drives_the_engine_and_close_withdraws_everything() {
    let host = FakeHost::default();
    let handle = open_zaps(&host, TARGET).unwrap();

    assert!(
        handle.projection_key().starts_with("nmp.zaps.summary."),
        "framework-owned per-read output key: {}",
        handle.projection_key()
    );
    assert_eq!(host.opened_filters.lock().unwrap().len(), 1, "one demand opened");
    assert_eq!(host.registry.live_count(), 1, "one live read in the shared registry");
    assert_eq!(
        host.output_key.lock().unwrap().as_deref(),
        Some(handle.projection_key()),
        "typed output installed under the handle's key"
    );

    // Live delivery folds into the typed output the shell will render.
    host.feed(&receipt("Z1", TARGET, Some("alice"), 10_000));
    let data = host.run_encoder().expect("output emits");
    let decoded = root_as_zap_summary_snapshot(&data.payload).unwrap();
    assert_eq!(decoded.total_msats(), 10_000);
    assert_eq!(decoded.zap_count(), 1);

    // Close withdraws the demand and tombstones the output — the engine no
    // longer tracks the read (no leak).
    assert!(close_zaps(&host, handle));
    assert_eq!(host.closed_interests.lock().unwrap().len(), 1, "the demand was withdrawn");
    assert!(host.output_key.lock().unwrap().is_none(), "output tombstoned");
    assert_eq!(host.registry.live_count(), 0, "no leak after close");
}

#[test]
fn open_zaps_rejects_a_malformed_target_event_id() {
    let host = FakeHost::default();
    let err = open_zaps(&host, "not-a-hex-id").unwrap_err();
    assert_eq!(err, crate::ZapTargetError::InvalidEventId);
    assert_eq!(host.registry.live_count(), 0, "no read was opened");
}
