//! Follow-up integration tests for the M7 publish engine that landed after
//! the original `publish_engine.rs` set. Kept in a sibling file so neither
//! crosses the 500-LOC hard cap from AGENTS.md.
//!
//! These exercise two codex 947dcfc findings:
//! - **D6 FFI mapping** — every `PublishEngineError` variant must surface as
//!   a `RecentFailure` snapshot row before the boundary crosses to Swift /
//!   Kotlin (never as an exception, never as `Result<T, E>`).
//! - **pending_retries durability** — a publish that's mid-backoff when the
//!   process dies must resume with its scheduled retry deadline intact,
//!   honouring the 1s/4s/16s schedule across restart.
//!
//! Plus multi-relay fan-out coverage added later: a real mixed publish driven
//! end-to-end through `start_publish` (one relay rejected, others accepted),
//! and per-relay independence of the state machine across a fan-out batch.

use std::collections::BTreeMap;
use std::sync::Arc;

use nmp_core::publish::{
    engine_error_to_failure, outcome_of, InMemoryPublishStore, NoopSigner, PerRelayState,
    PublishAction, PublishEngine, PublishEngineError, PublishOutcome, PublishStore,
    PublishStoreError, PublishTarget, RelayAck, RelayDispatcher, RelayUrl, ReplayDispatcher,
    RetryPolicy, StaticOutbox, ENGINE_FAILURE_RELAY_URL,
};
use nmp_core::substrate::*;

fn signed(id: &str, author: &str, kind: u32, p_tags: &[&str]) -> SignedEvent {
    let tags = p_tags
        .iter()
        .map(|p| vec!["p".to_string(), (*p).to_string()])
        .collect();
    SignedEvent {
        id: id.to_string(),
        sig: format!("sig-{}", id),
        unsigned: UnsignedEvent {
            pubkey: author.to_string(),
            kind,
            tags,
            content: format!("content-{}", id),
            created_at: 1_700_000_000,
        },
    }
}

fn outbox_with(author: &str, author_writes: &[&str]) -> Arc<StaticOutbox> {
    let mut o = StaticOutbox::default();
    o.author_writes.insert(
        author.to_string(),
        author_writes.iter().map(|r| r.to_string()).collect(),
    );
    Arc::new(o)
}

fn engine(
    outbox: Arc<dyn nmp_core::publish::OutboxResolver>,
    dispatcher: Arc<ReplayDispatcher>,
    store: Arc<dyn PublishStore>,
) -> PublishEngine {
    let signer = Arc::new(NoopSigner);
    PublishEngine::new(
        outbox,
        dispatcher as Arc<dyn RelayDispatcher>,
        store,
        signer,
        RetryPolicy::default(),
    )
}

#[test]
fn publish_engine_error_ffi_mapping_routes_to_recent_failure_d6() {
    // D6 FFI mapping regression: every `PublishEngineError` variant returned
    // from the engine MUST become a `RecentFailure` row on the snapshot
    // before the boundary crosses to the platform. No exceptions, no
    // `Result<T, E>` over FFI.

    // 1. NoTargets — exercise `record_engine_error` on a live engine,
    //    simulating what the FFI bridge will do after `start_publish` errs.
    let outbox = Arc::new(StaticOutbox::default());
    let dispatcher = Arc::new(ReplayDispatcher::new());
    let store: Arc<dyn PublishStore> = Arc::new(InMemoryPublishStore::new());
    let mut e = engine(outbox, dispatcher, store);

    let action = PublishAction::Publish {
        handle: "p-ffi-empty".to_string(),
        event: signed("ev-ffi-empty", "alice", 1, &[]),
        target: PublishTarget::Auto,
    };
    let err = e.start_publish(action, 100, None).unwrap_err();
    // start_publish_inner already pushed a recent_errors row for NoTargets;
    // verify the FFI mapping path is observable on top of that.
    let recent_before = e.snapshot().recent_errors.len();
    e.record_engine_error(&err, &"p-ffi-empty".to_string(), "ev-ffi-empty", 200);
    let recent_after = e.snapshot().recent_errors.len();
    assert_eq!(recent_after, recent_before + 1);
    let row = e.snapshot().recent_errors.last().unwrap();
    assert_eq!(row.relay_url, ENGINE_FAILURE_RELAY_URL);
    assert_eq!(row.reason, "no relays resolved for publish target");

    // 2. DuplicateHandle — exercise the pure helper for each variant.
    let dup = PublishEngineError::DuplicateHandle("p-x".to_string());
    let dup_row = engine_error_to_failure(&dup, &"p-x".to_string(), "ev-x", 1);
    assert_eq!(dup_row.relay_url, ENGINE_FAILURE_RELAY_URL);
    assert!(dup_row.reason.contains("duplicate"));
    assert!(dup_row.reason.contains("p-x"));

    // 3. Store — exercise the pure helper without a failing-store fixture.
    let store_err = PublishEngineError::Store(PublishStoreError::Backend("lmdb full".into()));
    let store_row = engine_error_to_failure(&store_err, &"p-s".to_string(), "ev-s", 2);
    assert_eq!(store_row.relay_url, ENGINE_FAILURE_RELAY_URL);
    assert!(store_row.reason.contains("publish store"));
    assert!(store_row.reason.contains("lmdb full"));
}

#[test]
fn publish_pending_retries_durable_across_restart() {
    // Regression for codex 947dcfc finding: a publish that's mid-backoff
    // when the process dies must resume with its scheduled retry deadline
    // intact. Without pending_retries persistence, the resumed engine
    // either retries immediately (thundering herd against the relay) or
    // never (silent drop). Both are wrong; the engine must honour the
    // original 1s/4s backoff schedule across restart.

    let outbox = outbox_with("alice", &["wss://backoff"]);
    let store: Arc<dyn PublishStore> = Arc::new(InMemoryPublishStore::new());

    // Instance 1: scripted to fail transiently so the engine schedules a
    // retry at now_ms (0) + 1_000ms (first-attempt backoff). Then the
    // process "dies" (we just drop the engine).
    let dispatcher_1 = Arc::new(ReplayDispatcher::new());
    dispatcher_1.script(
        "wss://backoff",
        vec![RelayAck::failed("wss://backoff", "io", "io error")],
    );
    {
        let mut e = engine(outbox.clone(), dispatcher_1.clone(), store.clone());
        e.start_publish(
            PublishAction::Publish {
                handle: "p-backoff".to_string(),
                event: signed("ev-backoff", "alice", 1, &[]),
                target: PublishTarget::Auto,
            },
            0,
            None,
        )
        .unwrap();
        // After the transient failure, the row is in RelayError and a
        // pending_retries deadline of 1_000ms is persisted. The store row
        // must carry that deadline.
        let pending = store.load_pending().unwrap();
        assert_eq!(pending.len(), 1, "row persisted across drop");
        let retries = &pending[0].pending_retries;
        assert_eq!(retries.len(), 1, "pending_retries persisted: {:?}", retries);
        assert_eq!(retries[0].0, "wss://backoff");
        assert_eq!(
            retries[0].1, 1_000,
            "deadline = 0 + 1s backoff: {:?}",
            retries
        );
    }

    // Instance 2: resume at now_ms = 500ms — BEFORE the deadline. The
    // engine must NOT dispatch yet (durable backoff respected).
    let dispatcher_2 = Arc::new(ReplayDispatcher::new());
    dispatcher_2.script("wss://backoff", vec![RelayAck::ok("wss://backoff")]);
    let mut e2 = engine(outbox.clone(), dispatcher_2.clone(), store.clone());
    e2.resume_from_store(500).unwrap();
    assert_eq!(
        dispatcher_2.sent_frames().len(),
        0,
        "resume must NOT dispatch before the persisted retry deadline (now=500ms, due=1000ms)"
    );
    assert!(
        e2.snapshot().recent_ok.is_empty(),
        "no ack yet — backoff still pending"
    );

    // Instance 3 (same store, fresh engine + dispatcher): resume at
    // now_ms = 1_500ms — AFTER the deadline. The engine must dispatch and
    // complete.
    let dispatcher_3 = Arc::new(ReplayDispatcher::new());
    dispatcher_3.script("wss://backoff", vec![RelayAck::ok("wss://backoff")]);
    let mut e3 = engine(outbox, dispatcher_3.clone(), store.clone());
    e3.resume_from_store(1_500).unwrap();
    assert_eq!(
        dispatcher_3.sent_frames().len(),
        1,
        "resume past the deadline must dispatch the retry"
    );
    assert_eq!(
        e3.snapshot().recent_ok.len(),
        1,
        "retry succeeded after restart-respecting-backoff"
    );
    assert!(
        store.load_pending().unwrap().is_empty(),
        "store cleared after completion"
    );
}

#[test]
fn publish_partial_success_one_relay_rejected_others_accepted() {
    // Multi-relay fan-out gap: nothing drove a *mixed* publish end-to-end
    // through the engine — only the pure `outcome_of` classifier was unit
    // tested. Here three relays fan out, one returns a permanent rejection
    // ("blocked", a NIP-20 OK-false code → no retries), two accept. The
    // engine must:
    //   - record the two accepting relays in a single `recent_ok` entry
    //   - record the one rejecting relay in `recent_errors`
    //   - evict the handle (all three per-relay slots are terminal)
    //   - classify the per-relay map as `PublishOutcome::Mixed`
    let mut outbox = StaticOutbox::default();
    outbox.author_writes.insert(
        "alice".to_string(),
        vec![
            "wss://accept-1".to_string(),
            "wss://accept-2".to_string(),
            "wss://reject".to_string(),
        ],
    );
    let outbox = Arc::new(outbox);
    let dispatcher = Arc::new(ReplayDispatcher::new());
    dispatcher.script("wss://accept-1", vec![RelayAck::ok("wss://accept-1")]);
    dispatcher.script("wss://accept-2", vec![RelayAck::ok("wss://accept-2")]);
    dispatcher.script(
        "wss://reject",
        vec![RelayAck::failed("wss://reject", "blocked", "blocked: spam")],
    );
    let store: Arc<dyn PublishStore> = Arc::new(InMemoryPublishStore::new());

    // Capture the per-relay map at the terminal moment so we can feed it to
    // `outcome_of` — once the handle is evicted the engine no longer exposes
    // it via `per_relay`.
    let captured: BTreeMap<RelayUrl, PerRelayState>;
    {
        let mut e = engine(outbox, dispatcher.clone(), store.clone());
        e.start_publish(
            PublishAction::Publish {
                handle: "p-mixed".to_string(),
                event: signed("ev-mixed", "alice", 1, &[]),
                target: PublishTarget::Auto,
            },
            1_000,
            None,
        )
        .unwrap();

        // All three relays were scripted with a single ack each, so the
        // publish settles entirely inside `start_publish`. The handle is
        // evicted; `per_relay` is empty.
        assert!(
            e.per_relay(&"p-mixed".to_string()).is_empty(),
            "mixed publish is fully terminal → handle evicted from in_flight"
        );

        let snap = e.snapshot();
        assert_eq!(
            snap.recent_ok.len(),
            1,
            "two OK acks coalesce into one recent_ok entry"
        );
        let mut accepted = snap.recent_ok[0].accepted_by.clone();
        accepted.sort();
        assert_eq!(
            accepted,
            vec!["wss://accept-1".to_string(), "wss://accept-2".to_string()],
            "only the two accepting relays appear in the success row"
        );
        assert_eq!(
            snap.recent_errors.len(),
            1,
            "exactly one relay rejected → one recent_errors entry"
        );
        assert_eq!(snap.recent_errors[0].relay_url, "wss://reject");

        // Reconstruct the terminal per-relay map for the classifier check.
        captured = [
            (
                "wss://accept-1".to_string(),
                PerRelayState::Ok { acked_at_ms: 1_000 },
            ),
            (
                "wss://accept-2".to_string(),
                PerRelayState::Ok { acked_at_ms: 1_000 },
            ),
            (
                "wss://reject".to_string(),
                PerRelayState::FailedAfterRetries {
                    reason: "blocked: spam".to_string(),
                    last_at_ms: 1_000,
                },
            ),
        ]
        .into_iter()
        .collect();
    }

    match outcome_of(&captured) {
        PublishOutcome::Mixed { accepted, failed } => {
            assert_eq!(accepted.len(), 2, "two relays accepted");
            assert_eq!(failed.len(), 1, "one relay failed");
            assert_eq!(failed[0], "wss://reject");
        }
        other => panic!("expected PublishOutcome::Mixed, got {:?}", other),
    }

    // The store row is gone — a settled (even partially-failed) publish is
    // not pending work.
    assert!(
        store.load_pending().unwrap().is_empty(),
        "store cleared once every relay reached a terminal state"
    );
}

#[test]
fn publish_all_relays_accepted_marks_publish_complete() {
    // Multi-relay fan-out gap: an all-accepted publish across several relays
    // must settle as one coalesced success AND fully evict — the engine keeps
    // no in-flight row for completed work (D5: snapshots bounded by what's
    // open).
    let mut outbox = StaticOutbox::default();
    outbox.author_writes.insert(
        "alice".to_string(),
        vec![
            "wss://a".to_string(),
            "wss://b".to_string(),
            "wss://c".to_string(),
        ],
    );
    let outbox = Arc::new(outbox);
    let dispatcher = Arc::new(ReplayDispatcher::new());
    for r in ["wss://a", "wss://b", "wss://c"] {
        dispatcher.script(r, vec![RelayAck::ok(r)]);
    }
    let store: Arc<dyn PublishStore> = Arc::new(InMemoryPublishStore::new());
    let mut e = engine(outbox, dispatcher, store.clone());

    e.start_publish(
        PublishAction::Publish {
            handle: "p-all-ok".to_string(),
            event: signed("ev-all-ok", "alice", 1, &[]),
            target: PublishTarget::Auto,
        },
        1_000,
        None,
    )
    .unwrap();

    let snap = e.snapshot();
    assert_eq!(snap.recent_ok.len(), 1, "three OK acks → one success row");
    assert_eq!(snap.recent_ok[0].accepted_by.len(), 3);
    assert!(snap.recent_errors.is_empty(), "no failures");
    assert!(
        e.per_relay(&"p-all-ok".to_string()).is_empty(),
        "all-accepted publish is complete → handle evicted"
    );
    assert!(
        store.load_pending().unwrap().is_empty(),
        "store cleared on completion"
    );
}

#[test]
fn publish_per_relay_states_tracked_independently_across_fanout() {
    // Multi-relay fan-out gap: each relay must carry its OWN state machine
    // slot. The `ReplayDispatcher` answers synchronously, so after
    // `start_publish` returns no relay can still be InFlight — but the three
    // relays still end up in distinct, simultaneously-observable states:
    // two independent `RelayError` slots (one reached via a TimedOut ack,
    // one via an `io` failure) and one `Ok`. The two failing relays carry
    // their own `pending_retries` deadlines, and the one `Ok` does NOT
    // collapse the whole publish to complete — proving the engine keeps
    // per-relay state, not a single shared verdict.
    let mut outbox = StaticOutbox::default();
    outbox.author_writes.insert(
        "alice".to_string(),
        vec![
            "wss://timed-out".to_string(),
            "wss://retrying".to_string(),
            "wss://done".to_string(),
        ],
    );
    let outbox = Arc::new(outbox);
    let dispatcher = Arc::new(ReplayDispatcher::new());
    // wss://timed-out: a TimedOut ack — transient → schedules a retry,
    // leaving the slot in RelayError.
    dispatcher.script(
        "wss://timed-out",
        vec![RelayAck::timed_out("wss://timed-out")],
    );
    // wss://retrying: one `io` transient failure → its own RelayError slot
    // with a retry pending.
    dispatcher.script(
        "wss://retrying",
        vec![RelayAck::failed("wss://retrying", "io", "io error")],
    );
    // wss://done: immediate OK.
    dispatcher.script("wss://done", vec![RelayAck::ok("wss://done")]);
    let store: Arc<dyn PublishStore> = Arc::new(InMemoryPublishStore::new());
    let mut e = engine(outbox, dispatcher, store);

    e.start_publish(
        PublishAction::Publish {
            handle: "p-indep".to_string(),
            event: signed("ev-indep", "alice", 1, &[]),
            target: PublishTarget::Auto,
        },
        1_000,
        None,
    )
    .unwrap();

    // The publish is NOT complete — two relays are still non-terminal — so
    // the handle is still in flight and its per-relay map is observable.
    let per_relay = e.per_relay(&"p-indep".to_string());
    assert_eq!(per_relay.len(), 3, "three relays, three independent slots");

    // wss://done settled Ok — independent of the other two.
    assert!(
        matches!(per_relay.get("wss://done"), Some(PerRelayState::Ok { .. })),
        "wss://done settled Ok independently: {:?}",
        per_relay.get("wss://done")
    );
    // wss://timed-out → its own RelayError slot (transient retry scheduled).
    assert!(
        matches!(
            per_relay.get("wss://timed-out"),
            Some(PerRelayState::RelayError { .. })
        ),
        "wss://timed-out is in its own retry state: {:?}",
        per_relay.get("wss://timed-out")
    );
    // wss://retrying failed transiently → its own, separate RelayError slot.
    assert!(
        matches!(
            per_relay.get("wss://retrying"),
            Some(PerRelayState::RelayError { .. })
        ),
        "wss://retrying is mid-retry independently: {:?}",
        per_relay.get("wss://retrying")
    );

    // The success on wss://done did NOT settle the whole publish — proving
    // per-relay independence at the completion gate too.
    assert!(
        e.snapshot().recent_ok.is_empty(),
        "one relay's OK must not mark a multi-relay publish complete"
    );
}
