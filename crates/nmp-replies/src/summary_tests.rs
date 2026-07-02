//! Concept-side proofs for `open_replies`: the door composes conventions,
//! admits true replies, emits typed output, and drives the ONE engine — with no
//! lifecycle code of its own (a fake host records the engine calls).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{KernelEvent, ObservedProjection};
use nmp_core::{ObservedProjectionId, ObservedProjectionSink};
use nmp_ownership::ProjectionRegistrationKey;
use nmp_read_session::{
    ReadHost, ReadOutputEncoder, ReadSessionBuild, ReadSessionId, ReadSessionRegistry,
    TeardownAction,
};

use super::generated::nmp::replies::root_as_reply_summary_snapshot;
use super::*;
use crate::read::reply_read_plans;
use crate::{ReplyProtocol, ReplyTarget};

const ROOT: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const REPLY_A: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const REPLY_B: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const MENTION: &str = "4444444444444444444444444444444444444444444444444444444444444444";
const AUTHOR: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn event(id: &str, kind: u32, tags: Vec<Vec<&str>>) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: AUTHOR.to_string(),
        kind,
        created_at: 1,
        tags: tags
            .into_iter()
            .map(|tag| tag.into_iter().map(str::to_string).collect())
            .collect(),
        content: "body".to_string(),
        relay_provenance: Vec::new(),
    }
}

fn note_target() -> ReplyTarget {
    ReplyTarget::event(ROOT, 1, Some(AUTHOR.to_string())).unwrap()
}

// ── The concept composition + admission (no engine involved) ────────────────

#[test]
fn a_note_target_composes_nip10_and_nip22_demands() {
    let plans = reply_read_plans(&note_target()).unwrap();
    assert_eq!(plans.len(), 2, "a plain note gets both conventions");
    assert_eq!(plans[0].protocol, ReplyProtocol::Nip10);
    assert_eq!(plans[1].protocol, ReplyProtocol::Nip22);
    // NIP-10 kind:1 #e demand + NIP-22 kind:1111 #E demand.
    assert!(plans[0].filter_json().contains("\"kinds\":[1]"));
    assert!(plans[1].filter_json().contains("\"kinds\":[1111]"));
}

#[test]
fn a_comment_target_composes_only_nip22() {
    let target = ReplyTarget::address("30023:pubkey:essay", 30023, None).unwrap();
    let plans = reply_read_plans(&target).unwrap();
    assert_eq!(plans.len(), 1, "non-note target: NIP-22 only");
    assert_eq!(plans[0].protocol, ReplyProtocol::Nip22);
}

#[test]
fn reducer_counts_true_replies_across_conventions_and_ignores_a_mention() {
    let plans = reply_read_plans(&note_target()).unwrap();
    let reducer = ReplySummaryProjection::new(ROOT.to_string(), plans);

    // A NIP-10 kind:1 direct reply.
    reducer.on_kernel_event(&event(REPLY_A, 1, vec![vec!["e", ROOT, "", "reply"]]));
    // A NIP-22 kind:1111 comment whose direct parent is the note.
    reducer.on_kernel_event(&event(
        REPLY_B,
        1111,
        vec![vec!["E", ROOT], vec!["K", "1"], vec!["e", ROOT], vec!["k", "1"]],
    ));
    // A bare #e mention (no reply marker) is NOT a reply.
    reducer.on_kernel_event(&event(MENTION, 1, vec![vec!["e", ROOT, "", "mention"]]));
    // Duplicate delivery of REPLY_A must not double count.
    reducer.on_kernel_event(&event(REPLY_A, 1, vec![vec!["e", ROOT, "", "reply"]]));

    let snapshot = reducer.snapshot();
    assert_eq!(snapshot.count, 2, "two distinct true replies, mention excluded");
    assert_eq!(snapshot.target_id, ROOT);
    assert_eq!(snapshot.reply_event_ids, vec![REPLY_A.to_string(), REPLY_B.to_string()]);
}

#[test]
fn typed_output_round_trips() {
    let snapshot = ReplySummarySnapshot {
        target_id: ROOT.to_string(),
        count: 2,
        reply_event_ids: vec![REPLY_A.to_string(), REPLY_B.to_string()],
    };
    let bytes = encode_reply_summary_snapshot(&snapshot);
    let decoded = root_as_reply_summary_snapshot(&bytes).unwrap();
    assert_eq!(decoded.schema_version(), REPLY_SUMMARY_SCHEMA_VERSION);
    assert_eq!(decoded.target_id(), Some(ROOT));
    assert_eq!(decoded.count(), 2);
    let ids: Vec<&str> = decoded.reply_event_ids().unwrap().iter().collect();
    assert_eq!(ids, vec![REPLY_A, REPLY_B]);
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
fn open_replies_drives_the_engine_and_close_withdraws_everything() {
    let host = FakeHost::new_default();
    let handle = open_replies(&host, note_target()).unwrap();

    assert!(
        handle.projection_key().starts_with("nmp.replies.summary."),
        "framework-owned per-read output key: {}",
        handle.projection_key()
    );
    assert_eq!(host.opened_filters.lock().unwrap().len(), 2, "two demands opened");
    assert_eq!(host.registry.live_count(), 1, "one live read in the shared registry");
    assert_eq!(
        host.output_key.lock().unwrap().as_deref(),
        Some(handle.projection_key()),
        "typed output installed under the handle's key"
    );

    // Live delivery folds into the typed output the shell will render.
    host.feed(&event(REPLY_A, 1, vec![vec!["e", ROOT, "", "reply"]]));
    let data = host.run_encoder().expect("output emits");
    let decoded = root_as_reply_summary_snapshot(&data.payload).unwrap();
    assert_eq!(decoded.count(), 1);

    // Close withdraws every demand and tombstones the output — reverse order,
    // once — and the engine no longer tracks the read (no leak).
    assert!(close_replies(&host, handle));
    assert_eq!(host.closed_interests.lock().unwrap().len(), 2, "both demands withdrawn");
    assert!(host.output_key.lock().unwrap().is_none(), "output tombstoned");
    assert_eq!(host.registry.live_count(), 0, "no leak after close");
}

impl FakeHost {
    fn new_default() -> Self {
        Self::default()
    }
}
