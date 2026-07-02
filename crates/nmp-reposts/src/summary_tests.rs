//! Concept-side proofs for `open_reposts`: the door composes the NIP-18
//! demand, admits true reposts (k-tag discriminated), folds deletes, emits
//! typed output, and drives the ONE engine — with no lifecycle code of its
//! own (a fake host records the engine calls).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{KernelEvent, ObservedProjection};
use nmp_core::{ObservedProjectionId, ObservedProjectionSink};
use nmp_nip18::{KIND_GENERIC_REPOST, KIND_REPOST};
use nmp_ownership::ProjectionRegistrationKey;
use nmp_read_session::{
    ReadHost, ReadOutputEncoder, ReadSessionBuild, ReadSessionId, ReadSessionRegistry,
    TeardownAction,
};

use super::generated::nmp::reposts::root_as_repost_summary_snapshot;
use super::*;
use crate::read::RepostReadPlan;
use crate::target::RepostTarget;

const TARGET: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const REPOST_A: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const REPOST_B: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BOB: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn event(id: &str, author: &str, kind: u32, tags: Vec<Vec<&str>>) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind,
        created_at: 1,
        tags: tags
            .into_iter()
            .map(|tag| tag.into_iter().map(str::to_string).collect())
            .collect(),
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

fn delete_event(id: &str, author: &str, deleted: &str) -> KernelEvent {
    event(id, author, KIND_DELETE, vec![vec!["e", deleted]])
}

fn target() -> RepostTarget {
    RepostTarget::note(TARGET).unwrap()
}

// ── The concept reducer (no engine involved) ────────────────────────────────

#[test]
fn reducer_counts_distinct_reposters_and_dedups_by_pubkey() {
    let plan = RepostReadPlan::new(&target());
    let reducer = RepostSummaryProjection::new(plan);

    reducer.on_kernel_event(&event(REPOST_A, ALICE, KIND_REPOST, vec![vec!["e", TARGET]]));
    reducer.on_kernel_event(&event(
        REPOST_B,
        BOB,
        KIND_GENERIC_REPOST,
        vec![vec!["e", TARGET], vec!["k", "1"]],
    ));
    // Alice reposts again (a second wrapper) — still counts once.
    reducer.on_kernel_event(&event(
        "r-alice-2",
        ALICE,
        KIND_REPOST,
        vec![vec!["e", TARGET]],
    ));

    let snapshot = reducer.snapshot();
    assert_eq!(snapshot.count, 2, "two distinct reposters, dedup by pubkey");
    assert_eq!(snapshot.target_id, TARGET);
    assert_eq!(
        snapshot.reposter_pubkeys,
        vec![ALICE.to_string(), BOB.to_string()]
    );
}

#[test]
fn reducer_ignores_generic_repost_of_a_different_target_kind() {
    let plan = RepostReadPlan::new(&target());
    let reducer = RepostSummaryProjection::new(plan);

    reducer.on_kernel_event(&event(
        REPOST_A,
        ALICE,
        KIND_GENERIC_REPOST,
        vec![vec!["e", TARGET], vec!["k", "30023"]],
    ));

    assert_eq!(reducer.snapshot().count, 0);
}

#[test]
fn same_author_delete_retracts_their_repost() {
    let plan = RepostReadPlan::new(&target());
    let reducer = RepostSummaryProjection::new(plan);

    reducer.on_kernel_event(&event(REPOST_A, ALICE, KIND_REPOST, vec![vec!["e", TARGET]]));
    reducer.on_kernel_event(&event(
        REPOST_B,
        BOB,
        KIND_GENERIC_REPOST,
        vec![vec!["e", TARGET], vec!["k", "1"]],
    ));
    assert_eq!(reducer.snapshot().count, 2);

    reducer.on_kernel_event(&delete_event("d1", ALICE, REPOST_A));

    let snapshot = reducer.snapshot();
    assert_eq!(snapshot.count, 1, "alice's retraction removes her repost");
    assert_eq!(snapshot.reposter_pubkeys, vec![BOB.to_string()]);
}

#[test]
fn foreign_delete_does_not_retract_a_repost() {
    let plan = RepostReadPlan::new(&target());
    let reducer = RepostSummaryProjection::new(plan);

    reducer.on_kernel_event(&event(REPOST_A, ALICE, KIND_REPOST, vec![vec!["e", TARGET]]));
    // Bob cannot delete Alice's repost wrapper.
    reducer.on_kernel_event(&delete_event("d1", BOB, REPOST_A));

    assert_eq!(reducer.snapshot().count, 1, "foreign delete is a no-op");
}

#[test]
fn retracting_one_of_two_reposts_from_the_same_author_keeps_them_counted() {
    let plan = RepostReadPlan::new(&target());
    let reducer = RepostSummaryProjection::new(plan);

    reducer.on_kernel_event(&event(REPOST_A, ALICE, KIND_REPOST, vec![vec!["e", TARGET]]));
    reducer.on_kernel_event(&event(
        "r-alice-2",
        ALICE,
        KIND_REPOST,
        vec![vec!["e", TARGET]],
    ));
    assert_eq!(reducer.snapshot().count, 1, "still one distinct reposter");

    reducer.on_kernel_event(&delete_event("d1", ALICE, REPOST_A));

    assert_eq!(
        reducer.snapshot().count,
        1,
        "alice's surviving second repost keeps her counted"
    );
}

#[test]
fn typed_output_round_trips() {
    let snapshot = RepostSummarySnapshot {
        target_id: TARGET.to_string(),
        count: 2,
        reposter_pubkeys: vec![ALICE.to_string(), BOB.to_string()],
    };
    let bytes = encode_repost_summary_snapshot(&snapshot);
    let decoded = root_as_repost_summary_snapshot(&bytes).unwrap();
    assert_eq!(decoded.schema_version(), REPOST_SUMMARY_SCHEMA_VERSION);
    assert_eq!(decoded.target_id(), Some(TARGET));
    assert_eq!(decoded.count(), 2);
    let pubkeys: Vec<&str> = decoded.reposter_pubkeys().unwrap().iter().collect();
    assert_eq!(pubkeys, vec![ALICE, BOB]);
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
fn open_reposts_drives_the_engine_and_close_withdraws_everything() {
    let host = FakeHost::default();
    let handle = open_reposts(&host, TARGET).unwrap();

    assert!(
        handle.projection_key().starts_with("nmp.reposts.summary."),
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
    host.feed(&event(REPOST_A, ALICE, KIND_REPOST, vec![vec!["e", TARGET]]));
    let data = host.run_encoder().expect("output emits");
    let decoded = root_as_repost_summary_snapshot(&data.payload).unwrap();
    assert_eq!(decoded.count(), 1);

    // Close withdraws the demand and tombstones the output — reverse order,
    // once — and the engine no longer tracks the read (no leak).
    assert!(close_reposts(&host, handle));
    assert_eq!(host.closed_interests.lock().unwrap().len(), 1, "the demand withdrawn");
    assert!(host.output_key.lock().unwrap().is_none(), "output tombstoned");
    assert_eq!(host.registry.live_count(), 0, "no leak after close");
}

#[test]
fn open_reposts_rejects_a_malformed_target() {
    let host = FakeHost::default();
    assert_eq!(
        open_reposts(&host, "not-hex").unwrap_err(),
        crate::target::RepostTargetError::InvalidEventId
    );
    assert_eq!(host.registry.live_count(), 0, "no session opened on error");
}
