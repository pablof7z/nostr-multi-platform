//! Concept-side proofs for `open_reactions`: the door composes the NIP-25
//! fold, admits kind:7 + kind:5 retractions, emits typed output, and drives
//! the ONE engine — with no lifecycle code of its own (a fake host records
//! the engine calls).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{KernelEvent, ObservedProjection};
use nmp_core::{ObservedProjectionId, ObservedProjectionSink};
use nmp_ownership::ProjectionRegistrationKey;
use nmp_read_session::{
    ReadHost, ReadInterestController, ReadOutputEncoder, ReadSessionBuild, ReadSessionId,
    ReadSessionRegistry, TeardownAction,
};

use super::*;

const TARGET: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const REACTION_A: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const REACTION_B: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const AUTHOR_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const AUTHOR_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn event(id: &str, author: &str, kind: u32, tags: Vec<Vec<&str>>, content: &str) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind,
        created_at: 1,
        tags: tags
            .into_iter()
            .map(|tag| tag.into_iter().map(str::to_string).collect())
            .collect(),
        content: content.to_string(),
        relay_provenance: Vec::new(),
    }
}

fn reaction(id: &str, author: &str, content: &str) -> KernelEvent {
    event(id, author, 7, vec![vec!["e", TARGET]], content)
}

fn delete(id: &str, author: &str, target_reaction_id: &str) -> KernelEvent {
    event(id, author, 5, vec![vec!["e", target_reaction_id]], "")
}

fn assert_delete_filter(filter: &str, deleted: &str, author: &str) {
    assert!(filter.contains(r#""kinds":[5]"#), "kind:5 filter: {filter}");
    assert!(
        filter.contains(&format!(r##""#e":["{deleted}"]"##)),
        "delete filter tags admitted reaction id {deleted}: {filter}"
    );
    assert!(
        filter.contains(&format!(r#""authors":["{author}"]"#)),
        "delete filter scopes to the reactor {author}: {filter}"
    );
}

// ── The concept composition (no engine involved) ────────────────────────────

#[test]
fn filter_targets_the_event_id() {
    let filter = reaction_filter_json(&ReactionTarget::event(TARGET).unwrap());
    assert!(filter.contains(TARGET));
}

#[test]
fn reducer_counts_and_groups_reactions_by_token() {
    let projection = ReactionAggregateProjection::new(None);
    projection.on_kernel_event(&reaction(REACTION_A, AUTHOR_A, "+"));
    projection.on_kernel_event(&reaction(REACTION_B, AUTHOR_B, "🔥"));
    // Duplicate delivery must not double count.
    projection.on_kernel_event(&reaction(REACTION_A, AUTHOR_A, "+"));

    let snapshot = reaction_summary_for(&projection, TARGET);
    assert_eq!(snapshot.total, 2);
    assert_eq!(snapshot.groups.len(), 2);
    // Each group carries its own distinct reactor pubkeys — the identity-free
    // membership fact the shell compares its active pubkey against.
    let plus = snapshot.groups.iter().find(|g| g.token == "+").unwrap();
    assert_eq!(plus.count, 1);
    assert_eq!(plus.reactor_pubkeys, vec![AUTHOR_A.to_string()]);
    let fire = snapshot.groups.iter().find(|g| g.token == "🔥").unwrap();
    assert_eq!(fire.count, 1);
    assert_eq!(fire.reactor_pubkeys, vec![AUTHOR_B.to_string()]);
}

#[test]
fn a_kind_5_delete_from_the_reactor_retracts_the_reaction() {
    let projection = ReactionAggregateProjection::new(None);
    projection.on_kernel_event(&reaction(REACTION_A, AUTHOR_A, "+"));
    projection.on_kernel_event(&reaction(REACTION_B, AUTHOR_B, "+"));
    assert_eq!(reaction_summary_for(&projection, TARGET).total, 2);

    // A delete from someone OTHER than the reactor must not retract it.
    projection.on_kernel_event(&delete("del-wrong", AUTHOR_B, REACTION_A));
    assert_eq!(reaction_summary_for(&projection, TARGET).total, 2);

    // A delete from the original reactor retracts exactly that reaction —
    // count AND the group's reactor membership both drop.
    projection.on_kernel_event(&delete("del-right", AUTHOR_A, REACTION_A));
    let snapshot = reaction_summary_for(&projection, TARGET);
    assert_eq!(snapshot.total, 1);
    assert_eq!(snapshot.groups.len(), 1);
    assert_eq!(
        snapshot.groups[0].reactor_pubkeys,
        vec![AUTHOR_B.to_string()]
    );
}

#[test]
fn typed_output_round_trips() {
    let snapshot = ReactionSummarySnapshot {
        target_id: TARGET.to_string(),
        total: 2,
        groups: vec![ReactionGroupSummary {
            token: "+".to_string(),
            count: 2,
            reactor_pubkeys: vec![AUTHOR_A.to_string(), AUTHOR_B.to_string()],
        }],
    };
    let bytes = encode_reaction_summary_snapshot(&snapshot);
    assert_eq!(
        crate::decode_reaction_summary_snapshot(&bytes).unwrap(),
        snapshot
    );
}

#[test]
fn public_decoder_rejects_malformed_payload() {
    assert_eq!(
        crate::decode_reaction_summary_snapshot(b"not-nrcs").unwrap_err(),
        "missing NRCS file identifier"
    );
}

#[test]
fn decode_reaction_summary_snapshot_round_trips_through_encode() {
    // #2900: a pure-Rust consumer (no UniFFI/codegen boundary) must be able
    // to turn the engine-emitted payload bytes back into the typed snapshot
    // using ONLY this crate's own public decode fn.
    let snapshot = ReactionSummarySnapshot {
        target_id: TARGET.to_string(),
        total: 2,
        groups: vec![ReactionGroupSummary {
            token: "+".to_string(),
            count: 2,
            reactor_pubkeys: vec![AUTHOR_A.to_string(), AUTHOR_B.to_string()],
        }],
    };
    let bytes = encode_reaction_summary_snapshot(&snapshot);
    let decoded = decode_reaction_summary_snapshot(&bytes).expect("valid payload decodes");
    assert_eq!(decoded, snapshot);
}

#[test]
fn decode_reaction_summary_snapshot_rejects_a_foreign_buffer() {
    let err = decode_reaction_summary_snapshot(&[0u8; 16]).unwrap_err();
    assert!(err.contains("NRCS"), "{err}");
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
fn open_reactions_drives_the_engine_and_close_withdraws_everything() {
    let host = FakeHost::default();
    let handle = open_reactions(&host, TARGET).unwrap();

    assert!(
        handle
            .projection_key()
            .starts_with("nmp.reactions.summary."),
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

    // Live delivery folds into the typed output the shell will render — and
    // the group's raw reactor pubkeys are what a shell compares its own
    // active-account pubkey against (identity-free viewer derivation).
    host.feed(&reaction(REACTION_A, AUTHOR_A, "+"));
    let data = host.run_encoder().expect("output emits");
    let decoded = crate::decode_reaction_summary_snapshot(&data.payload).unwrap();
    assert_eq!(decoded.total, 1);
    assert!(
        decoded.groups[0]
            .reactor_pubkeys
            .iter()
            .any(|pubkey| pubkey == AUTHOR_A),
        "shell membership check works"
    );

    let filters = host.opened_filters.lock().unwrap().clone();
    assert_eq!(
        filters.len(),
        2,
        "admitting the reaction opens the engine-owned delete demand"
    );
    assert_delete_filter(filters.last().unwrap(), REACTION_A, AUTHOR_A);

    // A retraction folds through the derived observer opened after the reaction
    // was admitted; no concept-owned subscription loop is involved.
    host.feed_latest(&delete(
        "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
        AUTHOR_A,
        REACTION_A,
    ));
    let data = host.run_encoder().expect("output emits");
    let decoded = crate::decode_reaction_summary_snapshot(&data.payload).unwrap();
    assert_eq!(decoded.total, 0);

    // Close withdraws the demand and tombstones the output — and the engine
    // no longer tracks the read (no leak).
    assert!(close_reactions(&host, handle));
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
fn into_parts_from_parts_round_trips_and_closes_without_a_handle_map() {
    let host = FakeHost::default();
    let handle = open_reactions(&host, TARGET).unwrap();
    let expected_key = handle.projection_key().to_string();

    // The bridge-lane round trip (#2899 Part A): decompose to scalar parts —
    // exactly what a UniFFI facade can carry across the FFI boundary — then
    // reconstruct the typed handle from those same parts, with no
    // facade-owned handle map in between.
    let (projection_key, handle_id) = handle.into_parts();
    assert_eq!(projection_key, expected_key);
    assert_ne!(handle_id, 0);

    let reconstructed = ReactionsReadHandle::from_parts(projection_key, handle_id);
    assert_eq!(reconstructed.projection_key(), expected_key);

    assert!(
        close_reactions(&host, reconstructed),
        "a handle reconstructed purely from scalar parts still closes the live read"
    );
    assert_eq!(host.registry.live_count(), 0, "no leak after close");

    // D6 idempotency: closing again via a FRESH handle reconstructed from the
    // very same scalar parts (e.g. a facade retrying a close after a dropped
    // response) is a safe no-op — never a panic, never a double-run teardown.
    let reclosed_from_same_parts = ReactionsReadHandle::from_parts(expected_key.clone(), handle_id);
    assert!(
        !close_reactions(&host, reclosed_from_same_parts),
        "re-closing via a fresh from_parts reconstruction of an already-closed \
         session is idempotent, not a panic"
    );
    assert_eq!(host.registry.live_count(), 0, "still no leak");
}

#[test]
fn open_reactions_rejects_a_malformed_target() {
    let host = FakeHost::default();
    assert_eq!(
        open_reactions(&host, "not-hex"),
        Err(ReactionTargetError::InvalidEventId)
    );
    assert_eq!(host.registry.live_count(), 0, "nothing opened on rejection");
}
