//! Concept-side proofs for `open_zaps`: the door compiles the demand, admits
//! validated zaps, aggregates raw per-sender totals, emits typed output, and
//! drives the ONE engine — with no lifecycle code and no viewer-identity
//! dependency of its own (a fake host records the engine calls).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{KernelEvent, ObservedProjection};
use nmp_core::{ObservedProjectionId, ObservedProjectionSink};
use nmp_nip09::KIND_DELETION;
use nmp_ownership::ProjectionRegistrationKey;
use nmp_read_session::{
    ReadHost, ReadInterestController, ReadOutputEncoder, ReadSessionBuild, ReadSessionId,
    ReadSessionRegistry, TeardownAction,
};

use super::generated::nmp::zaps::root_as_zap_summary_snapshot;
use super::*;
use crate::target::ZapTarget;

const TARGET: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const OTHER: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const RECEIPT_A: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const RECEIPT_B: &str = "4444444444444444444444444444444444444444444444444444444444444444";
const PROVIDER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER_PROVIDER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn target() -> ZapTarget {
    ZapTarget::event(TARGET).expect("valid hex64 id")
}

fn receipt(id: &str, target_id: &str, sender: Option<&str>, amount: u64) -> KernelEvent {
    receipt_by_provider(PROVIDER, id, target_id, sender, amount)
}

fn receipt_by_provider(
    provider: &str,
    id: &str,
    target_id: &str,
    sender: Option<&str>,
    amount: u64,
) -> KernelEvent {
    let sender_json = sender
        .map(|s| format!("\"pubkey\":\"{s}\",\"tags\":[[\"amount\",\"{amount}\"]]"))
        .unwrap_or_else(|| format!("\"tags\":[[\"amount\",\"{amount}\"]]"));
    KernelEvent {
        id: id.to_string(),
        author: provider.to_string(),
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

fn delete_event(id: &str, author: &str, deleted: &str) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: KIND_DELETION,
        created_at: 2,
        tags: vec![vec!["e".to_string(), deleted.to_string()]],
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

fn assert_delete_filter(filter: &str, deleted: &str, author: &str) {
    assert!(filter.contains(r#""kinds":[5]"#), "kind:5 filter: {filter}");
    assert!(
        filter.contains(&format!(r##""#e":["{deleted}"]"##)),
        "delete filter tags admitted receipt id {deleted}: {filter}"
    );
    assert!(
        filter.contains(&format!(r#""authors":["{author}"]"#)),
        "delete filter scopes to the receipt provider {author}: {filter}"
    );
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
fn reducer_retracts_provider_delete_and_ignores_foreign_delete() {
    let reducer = ZapSummaryProjection::new(target());
    reducer.on_kernel_event(&receipt(RECEIPT_A, TARGET, Some("alice"), 10_000));
    assert_eq!(reducer.snapshot().zap_count, 1);

    reducer.on_kernel_event(&delete_event(
        "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
        OTHER_PROVIDER,
        RECEIPT_A,
    ));
    assert_eq!(
        reducer.snapshot().zap_count,
        1,
        "foreign provider delete cannot retract the receipt"
    );

    reducer.on_kernel_event(&delete_event(
        "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
        PROVIDER,
        RECEIPT_A,
    ));
    let snapshot = reducer.snapshot();
    assert_eq!(
        snapshot.zap_count, 0,
        "provider delete retracts the receipt"
    );
    assert_eq!(snapshot.total_msats, 0);
}

#[test]
fn a_caller_derives_viewer_zapped_from_the_raw_zappers_list() {
    // This concept exposes no viewer-relative field (per #2777/#2758 review):
    // a shell membership-checks its own pubkey against `zappers` itself.
    let reducer = ZapSummaryProjection::new(target());
    reducer.on_kernel_event(&receipt("Z1", TARGET, Some("alice"), 10_000));
    reducer.on_kernel_event(&receipt("Z2", TARGET, Some("bob"), 20_000));

    let snapshot = reducer.snapshot();
    let viewer_zapped = snapshot
        .zappers
        .iter()
        .any(|z| z.pubkey.as_deref() == Some("alice"));
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

#[test]
fn decode_zap_summary_snapshot_round_trips_through_encode() {
    // #2900: a pure-Rust consumer (no UniFFI/codegen boundary) must be able
    // to turn the engine-emitted payload bytes back into the typed snapshot
    // using ONLY this crate's own public decode fn.
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
    let decoded = decode_zap_summary_snapshot(&bytes).expect("valid payload decodes");
    assert_eq!(decoded, snapshot);
}

#[test]
fn decode_zap_summary_snapshot_rejects_a_foreign_buffer() {
    let err = decode_zap_summary_snapshot(&[0u8; 16]).unwrap_err();
    assert!(err.contains("NZSM"), "{err}");
}

// ── The door drives the engine end-to-end (fake host) ───────────────────────

#[derive(Default)]
struct FakeHost {
    registry: ReadSessionRegistry,
    observers: Arc<Mutex<Vec<Arc<dyn ObservedProjectionSink>>>>,
    encoder: Mutex<Option<ReadOutputEncoder>>,
    output_key: Arc<Mutex<Option<String>>>,
    opened_filters: Arc<Mutex<Vec<String>>>,
    closed_interests: Arc<Mutex<Vec<u64>>>,
    next_interest: Arc<AtomicU64>,
}

impl FakeHost {
    fn run_encoder(&self) -> Option<nmp_core::TypedProjectionData> {
        self.encoder.lock().unwrap().as_ref().and_then(|e| e())
    }
    fn feed(&self, event: &KernelEvent) {
        self.feed_observer(0, event);
    }
    fn feed_latest(&self, event: &KernelEvent) {
        let Some(index) = self.observers.lock().unwrap().len().checked_sub(1) else {
            return;
        };
        self.feed_observer(index, event);
    }
    fn feed_observer(&self, index: usize, event: &KernelEvent) {
        let observer = self.observers.lock().unwrap().get(index).cloned();
        if let Some(obs) = observer {
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
        self.observers
            .lock()
            .unwrap()
            .push(Arc::clone(&decl.observer));
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
    fn close_read_session_by_projection_key(&self, projection_key: &str) -> bool {
        self.registry.close_by_projection_key(projection_key)
    }
    fn read_interest_controller(&self) -> Option<ReadInterestController> {
        let observers = Arc::clone(&self.observers);
        let opened_filters = Arc::clone(&self.opened_filters);
        let next_interest = Arc::clone(&self.next_interest);
        let open = move |decl: ObservedProjection| {
            observers.lock().unwrap().push(Arc::clone(&decl.observer));
            opened_filters.lock().unwrap().push(decl.filter_json);
            ObservedProjectionId(next_interest.fetch_add(1, Ordering::Relaxed) + 1)
        };
        let closed = Arc::clone(&self.closed_interests);
        let close = move |id: ObservedProjectionId| {
            closed.lock().unwrap().push(id.0);
        };
        Some(ReadInterestController::new(open, close))
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
    assert_eq!(
        host.opened_filters.lock().unwrap().len(),
        1,
        "one demand opened"
    );
    assert_eq!(
        host.registry.live_count(),
        1,
        "one live read in the shared registry"
    );
    assert_eq!(
        host.output_key.lock().unwrap().as_deref(),
        Some(handle.projection_key()),
        "typed output installed under the handle's key"
    );

    // Live delivery folds into the typed output the shell will render.
    host.feed(&receipt(RECEIPT_A, TARGET, Some("alice"), 10_000));
    let data = host.run_encoder().expect("output emits");
    let decoded = root_as_zap_summary_snapshot(&data.payload).unwrap();
    assert_eq!(decoded.total_msats(), 10_000);
    assert_eq!(decoded.zap_count(), 1);

    // Close withdraws the demand and tombstones the output — the engine no
    // longer tracks the read (no leak).
    assert!(close_zaps(&host, handle));
    assert_eq!(
        host.closed_interests.lock().unwrap().len(),
        2,
        "primary and derived demands withdrawn"
    );
    assert!(
        host.output_key.lock().unwrap().is_none(),
        "output tombstoned"
    );
    assert_eq!(host.registry.live_count(), 0, "no leak after close");
}

#[test]
fn derived_delete_demand_routes_zap_receipt_delete_discovered_after_open() {
    let host = FakeHost::default();
    let handle = open_zaps(&host, TARGET).unwrap();
    assert_eq!(
        host.opened_filters.lock().unwrap().len(),
        1,
        "open starts with the static zap-receipt demand only"
    );

    host.feed(&receipt(RECEIPT_B, TARGET, Some("alice"), 10_000));
    let filters = host.opened_filters.lock().unwrap().clone();
    assert_eq!(
        filters.len(),
        2,
        "admitting the receipt opens the engine-owned delete demand"
    );
    assert_delete_filter(filters.last().unwrap(), RECEIPT_B, PROVIDER);

    host.feed_latest(&delete_event(
        "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
        PROVIDER,
        RECEIPT_B,
    ));
    let data = host.run_encoder().expect("output emits");
    let decoded = root_as_zap_summary_snapshot(&data.payload).unwrap();
    assert_eq!(
        decoded.zap_count(),
        0,
        "delete delivered through the derived demand retracts the receipt"
    );
    assert_eq!(decoded.total_msats(), 0);

    assert!(close_zaps(&host, handle));
}

#[test]
fn into_parts_from_parts_round_trips_and_closes_without_a_handle_map() {
    let host = FakeHost::default();
    let handle = open_zaps(&host, TARGET).unwrap();
    let expected_key = handle.projection_key().to_string();

    // The bridge-lane round trip (#2899 Part A): decompose to scalar parts —
    // exactly what a UniFFI facade can carry across the FFI boundary — then
    // reconstruct the typed handle from those same parts, with no
    // facade-owned handle map in between.
    let (projection_key, handle_id) = handle.into_parts();
    assert_eq!(projection_key, expected_key);
    assert_ne!(handle_id, 0);

    let reconstructed = ZapsReadHandle::from_parts(projection_key, handle_id);
    assert_eq!(reconstructed.projection_key(), expected_key);

    assert!(
        close_zaps(&host, reconstructed),
        "a handle reconstructed purely from scalar parts still closes the live read"
    );
    assert_eq!(host.registry.live_count(), 0, "no leak after close");

    // D6 idempotency: closing again via a FRESH handle reconstructed from the
    // very same scalar parts (e.g. a facade retrying a close after a dropped
    // response) is a safe no-op — never a panic, never a double-run teardown.
    let reclosed_from_same_parts = ZapsReadHandle::from_parts(expected_key.clone(), handle_id);
    assert!(
        !close_zaps(&host, reclosed_from_same_parts),
        "re-closing via a fresh from_parts reconstruction of an already-closed \
         session is idempotent, not a panic"
    );
    assert_eq!(host.registry.live_count(), 0, "still no leak");
}

#[test]
fn open_zaps_rejects_a_malformed_target_event_id() {
    let host = FakeHost::default();
    let err = open_zaps(&host, "not-a-hex-id").unwrap_err();
    assert_eq!(err, crate::ZapTargetError::InvalidEventId);
    assert_eq!(host.registry.live_count(), 0, "no read was opened");
}
