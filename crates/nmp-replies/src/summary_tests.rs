//! Concept-side proofs for `open_replies`: the door composes conventions,
//! admits true replies, emits typed output, and drives the ONE engine — with no
//! lifecycle code of its own (a fake host records the engine calls).

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

use super::*;
use crate::read::reply_read_plans;
use crate::{ReplyProtocol, ReplyTarget};

const ROOT: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const REPLY_A: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const REPLY_B: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const MENTION: &str = "4444444444444444444444444444444444444444444444444444444444444444";
const AUTHOR: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const AUTHOR_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn event(id: &str, kind: u32, tags: Vec<Vec<&str>>) -> KernelEvent {
    event_by(AUTHOR, id, kind, tags)
}

fn event_by(author: &str, id: &str, kind: u32, tags: Vec<Vec<&str>>) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
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

fn delete_event(id: &str, author: &str, deleted: &str) -> KernelEvent {
    event_by(author, id, KIND_DELETION, vec![vec!["e", deleted]])
}

fn assert_delete_filter(filter: &str, deleted: &str, author: &str) {
    assert!(filter.contains(r#""kinds":[5]"#), "kind:5 filter: {filter}");
    assert!(
        filter.contains(&format!(r##""#e":["{deleted}"]"##)),
        "delete filter tags admitted reply id {deleted}: {filter}"
    );
    assert!(
        filter.contains(&format!(r#""authors":["{author}"]"#)),
        "delete filter scopes to the reply author {author}: {filter}"
    );
}

fn note_target() -> ReplyTarget {
    ReplyTarget::event(ROOT, 1, Some(AUTHOR.to_string())).unwrap()
}

/// A top-level kind:1111 NIP-22 comment target, decoded through the real
/// FFI-marshalable input ([`crate::decode_and_validate_reply_target`]) rather
/// than constructed by hand — the marshal's `Comment` variant is the ONLY
/// correct way to supply a kind:1111 target (#2899 Part A; see
/// `target_tests.rs` for the rejection/decode unit proofs).
fn comment_target_via_marshal(comment_id: &str, root_id: &str) -> ReplyTarget {
    let json = format!(
        r#"{{"target_type":"comment","event_id":"{comment_id}","author_pubkey":"{AUTHOR}","created_at":1,"tags":[["E","{root_id}"],["K","1"],["e","{root_id}"],["k","1"]],"content":"top-level comment"}}"#
    );
    crate::decode_and_validate_reply_target(&json).unwrap()
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
        vec![
            vec!["E", ROOT],
            vec!["K", "1"],
            vec!["e", ROOT],
            vec!["k", "1"],
        ],
    ));
    // A bare #e mention (no reply marker) is NOT a reply.
    reducer.on_kernel_event(&event(MENTION, 1, vec![vec!["e", ROOT, "", "mention"]]));
    // Duplicate delivery of REPLY_A must not double count.
    reducer.on_kernel_event(&event(REPLY_A, 1, vec![vec!["e", ROOT, "", "reply"]]));

    let snapshot = reducer.snapshot();
    assert_eq!(
        snapshot.count, 2,
        "two distinct true replies, mention excluded"
    );
    assert_eq!(snapshot.target_id, ROOT);
    assert_eq!(
        snapshot.reply_event_ids,
        vec![REPLY_A.to_string(), REPLY_B.to_string()]
    );
}

#[test]
fn reducer_retracts_same_author_delete_and_ignores_foreign_delete() {
    let plans = reply_read_plans(&note_target()).unwrap();
    let reducer = ReplySummaryProjection::new(ROOT.to_string(), plans);

    reducer.on_kernel_event(&event(REPLY_A, 1, vec![vec!["e", ROOT, "", "reply"]]));
    assert_eq!(reducer.snapshot().count, 1);

    reducer.on_kernel_event(&delete_event(
        "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
        AUTHOR_B,
        REPLY_A,
    ));
    assert_eq!(
        reducer.snapshot().count,
        1,
        "foreign delete cannot retract a reply"
    );

    reducer.on_kernel_event(&delete_event(
        "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
        AUTHOR,
        REPLY_A,
    ));
    assert_eq!(
        reducer.snapshot().count,
        0,
        "same-author delete retracts the reply"
    );
}

#[test]
fn typed_output_round_trips() {
    let snapshot = ReplySummarySnapshot {
        target_id: ROOT.to_string(),
        count: 2,
        reply_event_ids: vec![REPLY_A.to_string(), REPLY_B.to_string()],
    };
    let bytes = encode_reply_summary_snapshot(&snapshot);
    assert_eq!(
        crate::decode_reply_summary_snapshot(&bytes).unwrap(),
        snapshot
    );
}

#[test]
fn public_decoder_rejects_malformed_payload() {
    assert_eq!(
        crate::decode_reply_summary_snapshot(b"not-nrsm").unwrap_err(),
        "missing NRSM file identifier"
    );
}

#[test]
fn decode_reply_summary_snapshot_round_trips_through_encode() {
    // #2900: a pure-Rust consumer (no UniFFI/codegen boundary) must be able
    // to turn the engine-emitted payload bytes back into the typed snapshot
    // using ONLY this crate's own public decode fn.
    let snapshot = ReplySummarySnapshot {
        target_id: ROOT.to_string(),
        count: 2,
        reply_event_ids: vec![REPLY_A.to_string(), REPLY_B.to_string()],
    };
    let bytes = encode_reply_summary_snapshot(&snapshot);
    let decoded = decode_reply_summary_snapshot(&bytes).expect("valid payload decodes");
    assert_eq!(decoded, snapshot);
}

#[test]
fn decode_reply_summary_snapshot_rejects_a_foreign_buffer() {
    let err = decode_reply_summary_snapshot(&[0u8; 16]).unwrap_err();
    assert!(err.contains("NRSM"), "{err}");
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
fn open_replies_drives_the_engine_and_close_withdraws_everything() {
    let host = FakeHost::new_default();
    let handle = open_replies(&host, note_target()).unwrap();

    assert!(
        handle.projection_key().starts_with("nmp.replies.summary."),
        "framework-owned per-read output key: {}",
        handle.projection_key()
    );
    assert_eq!(
        host.opened_filters.lock().unwrap().len(),
        2,
        "two demands opened"
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
    host.feed(&event(REPLY_A, 1, vec![vec!["e", ROOT, "", "reply"]]));
    let data = host.run_encoder().expect("output emits");
    let decoded = crate::decode_reply_summary_snapshot(&data.payload).unwrap();
    assert_eq!(decoded.count, 1);

    // Close withdraws every demand and tombstones the output — reverse order,
    // once — and the engine no longer tracks the read (no leak).
    assert!(close_replies(&host, handle));
    assert_eq!(
        host.closed_interests.lock().unwrap().len(),
        3,
        "primary and derived demands withdrawn"
    );
    assert!(
        host.output_key.lock().unwrap().is_none(),
        "output tombstoned"
    );
    assert_eq!(host.registry.live_count(), 0, "no leak after close");
}

#[test]
fn open_replies_drives_the_engine_for_a_kind_1111_comment_target_from_the_marshal() {
    // The load-bearing marshal proof (#2899 DERISK refocus): a kind:1111
    // target decoded via the FFI-marshalable `Comment` variant must drive
    // `open_replies` exactly like a hand-built `ReplyTarget::Comment`, with a
    // single NIP-22-only demand (no NIP-10 demand — a comment is never
    // `is_nip10`) scoped to the comment's OWN event id as the direct-parent
    // query.
    const COMMENT_ID: &str = "5555555555555555555555555555555555555555555555555555555555555555";
    let host = FakeHost::new_default();
    let target = comment_target_via_marshal(COMMENT_ID, ROOT);
    let handle = open_replies(&host, target).unwrap();

    let filters = host.opened_filters.lock().unwrap().clone();
    assert_eq!(
        filters.len(),
        1,
        "a comment target composes NIP-22 only, never NIP-10"
    );
    assert!(filters[0].contains("\"kinds\":[1111]"), "{}", filters[0]);
    assert!(
        filters[0].contains(&format!(r##""#e":["{COMMENT_ID}"]"##)),
        "queries by the comment's own id as the direct-parent scope: {}",
        filters[0]
    );

    // A real kind:1111 reply to that comment (its lowercase `e` parent tag
    // names the comment, its uppercase `E` root tag mirrors the thread root)
    // is admitted; a reply to some OTHER comment on the same thread is not.
    let reply_to_comment = event_by(
        AUTHOR_B,
        "6666666666666666666666666666666666666666666666666666666666666666",
        1111,
        vec![
            vec!["E", ROOT],
            vec!["K", "1"],
            vec!["e", COMMENT_ID],
            vec!["k", "1111"],
        ],
    );
    let reply_to_a_sibling = event_by(
        AUTHOR_B,
        "7777777777777777777777777777777777777777777777777777777777777777",
        1111,
        vec![
            vec!["E", ROOT],
            vec!["K", "1"],
            vec![
                "e",
                "8888888888888888888888888888888888888888888888888888888888888888",
            ],
            vec!["k", "1111"],
        ],
    );
    host.feed(&reply_to_comment);
    host.feed(&reply_to_a_sibling);

    let data = host.run_encoder().expect("output emits");
    let decoded = crate::decode_reply_summary_snapshot(&data.payload).unwrap();
    assert_eq!(
        decoded.count, 1,
        "only the reply that names the comment as its direct parent counts"
    );
    assert_eq!(decoded.target_id, COMMENT_ID);

    assert!(close_replies(&host, handle));
    assert_eq!(host.registry.live_count(), 0, "no leak after close");
}

#[test]
fn into_parts_from_parts_round_trips_and_closes_without_a_handle_map() {
    let host = FakeHost::new_default();
    let handle = open_replies(&host, note_target()).unwrap();
    let expected_key = handle.projection_key().to_string();

    // The bridge-lane round trip (#2899 Part A): decompose to scalar parts —
    // exactly what a UniFFI facade can carry across the FFI boundary — then
    // reconstruct the typed handle from those same parts, with no
    // facade-owned handle map in between.
    let (projection_key, handle_id) = handle.into_parts();
    assert_eq!(projection_key, expected_key);
    assert_ne!(handle_id, 0);

    let reconstructed = RepliesReadHandle::from_parts(projection_key, handle_id);
    assert_eq!(reconstructed.projection_key(), expected_key);

    assert!(
        close_replies(&host, reconstructed),
        "a handle reconstructed purely from scalar parts still closes the live read"
    );
    assert_eq!(host.registry.live_count(), 0, "no leak after close");

    // D6 idempotency: closing again via a FRESH handle reconstructed from the
    // very same scalar parts (e.g. a facade retrying a close after a dropped
    // response) is a safe no-op — never a panic, never a double-run teardown.
    let reclosed_from_same_parts = RepliesReadHandle::from_parts(expected_key.clone(), handle_id);
    assert!(
        !close_replies(&host, reclosed_from_same_parts),
        "re-closing via a fresh from_parts reconstruction of an already-closed \
         session is idempotent, not a panic"
    );
    assert_eq!(host.registry.live_count(), 0, "still no leak");
}

#[test]
fn derived_delete_demand_routes_reply_delete_discovered_after_open() {
    let host = FakeHost::new_default();
    let handle = open_replies(&host, note_target()).unwrap();
    assert_eq!(
        host.opened_filters.lock().unwrap().len(),
        2,
        "open starts with the static NIP-10 and NIP-22 reply demands"
    );

    host.feed(&event(REPLY_A, 1, vec![vec!["e", ROOT, "", "reply"]]));
    let filters = host.opened_filters.lock().unwrap().clone();
    assert_eq!(
        filters.len(),
        3,
        "admitting the reply opens the engine-owned delete demand"
    );
    assert_delete_filter(filters.last().unwrap(), REPLY_A, AUTHOR);

    host.feed_latest(&delete_event(
        "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
        AUTHOR,
        REPLY_A,
    ));
    let data = host.run_encoder().expect("output emits");
    let decoded = crate::decode_reply_summary_snapshot(&data.payload).unwrap();
    assert_eq!(
        decoded.count, 0,
        "delete delivered through the derived demand retracts the reply"
    );

    assert!(close_replies(&host, handle));
}

impl FakeHost {
    fn new_default() -> Self {
        Self::default()
    }
}
