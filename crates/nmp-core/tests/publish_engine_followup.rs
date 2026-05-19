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

use std::sync::Arc;

use nmp_core::publish::{
    engine_error_to_failure, InMemoryPublishStore, NoopSigner, PublishAction, PublishEngine,
    PublishEngineError, PublishStore, PublishStoreError, PublishTarget, RelayAck, RelayDispatcher,
    ReplayDispatcher, RetryPolicy, StaticOutbox, ENGINE_FAILURE_RELAY_URL,
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
    let err = e.start_publish(action, 100).unwrap_err();
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
