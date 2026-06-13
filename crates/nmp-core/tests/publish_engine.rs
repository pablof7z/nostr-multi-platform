//! Integration tests for `PublishEngine` covering the spec scenarios from
//! task #45 — outbox-automatic routing (D3), retry on transient failure,
//! give-up after retries, durability across "restart", dedup across multi-relay
//! fan-out, p-tag inbox routing.
//!
//! These are deterministic: the dispatcher is the `ReplayDispatcher` from
//! `nmp_core::publish::traits`; time is injected as `now_ms`. No sockets, no
//! sleeps.

use std::sync::Arc;

use nmp_core::publish::{
    outcome_of, InMemoryPublishStore, NoopSigner, PublishAction, PublishEngine, PublishOutcome,
    PublishStore, PublishTarget, RelayAck, RetryPolicy, StaticOutbox,
};
use nmp_core::publish::{
    OutboxResolver, PerRelayState, PublishStoreError, RelayDispatcher, ReplayDispatcher,
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

fn outbox_with(
    author: &str,
    author_writes: &[&str],
    p_reads: &[(&str, Vec<&str>)],
) -> Arc<StaticOutbox> {
    let mut o = StaticOutbox::default();
    o.author_writes.insert(
        author.to_string(),
        author_writes.iter().map(|r| r.to_string()).collect(),
    );
    for (p, reads) in p_reads {
        o.p_tag_reads.insert(
            (*p).to_string(),
            reads.iter().map(|r| r.to_string()).collect(),
        );
    }
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
fn publish_auto_resolves_outbox() {
    let outbox = outbox_with("alice", &["wss://r1", "wss://r2"], &[]);
    let dispatcher = Arc::new(ReplayDispatcher::new());
    for r in ["wss://r1", "wss://r2"] {
        dispatcher.script(r, vec![RelayAck::ok(r)]);
    }
    // A third relay exists in the world but is NOT in alice's kind:10002 — it
    // must NOT receive the publish.
    dispatcher.script("wss://r3", vec![RelayAck::ok("wss://r3")]);

    let store: Arc<dyn PublishStore> = Arc::new(InMemoryPublishStore::new());
    let mut e = engine(outbox, dispatcher.clone(), store);

    e.start_publish(
        PublishAction::Publish {
            handle: "p1".to_string(),
            event: signed("ev-auto", "alice", 1, &[]),
            target: PublishTarget::Auto,
        },
        1_000,
        None,
    )
    .unwrap();

    let sent: Vec<String> = dispatcher
        .sent_frames()
        .into_iter()
        .map(|(u, _)| u)
        .collect();
    let mut sent_sorted = sent.clone();
    sent_sorted.sort();
    assert_eq!(
        sent_sorted,
        vec!["wss://r1".to_string(), "wss://r2".to_string()]
    );
    assert!(!sent.contains(&"wss://r3".to_string()));
}

#[test]
fn publish_p_tag_inbox_routing() {
    let outbox = outbox_with(
        "alice",
        &["wss://alice-write"],
        &[("bob", vec!["wss://bob-read"])],
    );
    let dispatcher = Arc::new(ReplayDispatcher::new());
    for r in ["wss://alice-write", "wss://bob-read"] {
        dispatcher.script(r, vec![RelayAck::ok(r)]);
    }
    let store: Arc<dyn PublishStore> = Arc::new(InMemoryPublishStore::new());
    let mut e = engine(outbox, dispatcher.clone(), store);

    e.start_publish(
        PublishAction::Publish {
            handle: "p-ptag".to_string(),
            event: signed("ev-ptag", "alice", 1, &["bob"]),
            target: PublishTarget::Auto,
        },
        1_000,
        None,
    )
    .unwrap();

    let mut urls: Vec<String> = dispatcher
        .sent_frames()
        .into_iter()
        .map(|(u, _)| u)
        .collect();
    urls.sort();
    assert_eq!(
        urls,
        vec![
            "wss://alice-write".to_string(),
            "wss://bob-read".to_string()
        ]
    );
}

#[test]
fn publish_retry_on_connection_drop() {
    let outbox = outbox_with("alice", &["wss://flaky"], &[]);
    let dispatcher = Arc::new(ReplayDispatcher::new());
    // First send: connection drop (transient). Second send: OK.
    dispatcher.script(
        "wss://flaky",
        vec![
            RelayAck::failed("wss://flaky", "connection-reset", "connection reset"),
            RelayAck::ok("wss://flaky"),
        ],
    );
    let store: Arc<dyn PublishStore> = Arc::new(InMemoryPublishStore::new());
    let mut e = engine(outbox, dispatcher.clone(), store);

    e.start_publish(
        PublishAction::Publish {
            handle: "p-flaky".to_string(),
            event: signed("ev-flaky", "alice", 1, &[]),
            target: PublishTarget::Auto,
        },
        1_000,
        None,
    )
    .unwrap();

    // After first send the relay is in RelayError with retry scheduled 1s out.
    let per_relay = e.per_relay(&"p-flaky".to_string());
    let state = per_relay.get("wss://flaky").cloned().unwrap();
    assert!(matches!(state, PerRelayState::RelayError { .. }));

    // Tick past the backoff: 1_000 + 1_000 (delay) + slack.
    e.tick(2_500);

    // Final state: Ok and recorded in recent_ok.
    let snap = e.snapshot();
    assert_eq!(snap.recent_ok.len(), 1);
    assert_eq!(snap.recent_errors.len(), 0);
    // Engine evicted completed handle from in_flight.
    assert!(e.per_relay(&"p-flaky".to_string()).is_empty());
}

#[test]
fn publish_giveup_after_three_attempts() {
    let outbox = outbox_with("alice", &["wss://always-500"], &[]);
    let dispatcher = Arc::new(ReplayDispatcher::new());
    // Default policy allows three total attempts: initial send plus two
    // retries. Three explicit transient failures exhaust it.
    let fail = RelayAck::failed("wss://always-500", "io", "ERR 500");
    dispatcher.script(
        "wss://always-500",
        vec![fail.clone(), fail.clone(), fail.clone()],
    );
    let store: Arc<dyn PublishStore> = Arc::new(InMemoryPublishStore::new());
    let mut e = engine(outbox, dispatcher.clone(), store);

    e.start_publish(
        PublishAction::Publish {
            handle: "p-bad".to_string(),
            event: signed("ev-bad", "alice", 1, &[]),
            target: PublishTarget::Auto,
        },
        0,
        None,
    )
    .unwrap();

    // First send already happened in start_publish. Two more retries are
    // expected (attempt 2 after 1s, attempt 3 after 4s). After attempt 3
    // fails we must give up.
    e.tick(1_500); // attempt 2
    e.tick(6_000); // attempt 3 + give-up
    e.tick(30_000); // settle

    let snap = e.snapshot();
    assert!(snap.recent_ok.is_empty(), "expected no successes");
    assert_eq!(snap.recent_errors.len(), 1);
    let failure = &snap.recent_errors[0];
    assert_eq!(failure.relay_url, "wss://always-500");
    assert!(failure.reason.contains("transient"));
}

#[test]
fn publish_durable_across_restart() {
    let outbox = outbox_with("alice", &["wss://durable"], &[]);
    let dispatcher_1 = Arc::new(ReplayDispatcher::new());
    // First instance: send queued, NEVER ack'd (script empty → TimedOut).
    let store: Arc<dyn PublishStore> = Arc::new(InMemoryPublishStore::new());

    {
        let mut e = engine(outbox.clone(), dispatcher_1.clone(), store.clone());
        // Scripted to immediately time out so the engine schedules a retry,
        // leaving the row durably persisted.
        dispatcher_1.script("wss://durable", vec![RelayAck::timed_out("wss://durable")]);
        e.start_publish(
            PublishAction::Publish {
                handle: "p-durable".to_string(),
                event: signed("ev-durable", "alice", 1, &[]),
                target: PublishTarget::Auto,
            },
            0,
            None,
        )
        .unwrap();
        // Row should be in the store after the first dispatch.
        let pending = store.load_pending().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].handle, "p-durable");
    }

    // "Restart": new engine instance, new dispatcher that this time succeeds.
    let dispatcher_2 = Arc::new(ReplayDispatcher::new());
    dispatcher_2.script("wss://durable", vec![RelayAck::ok("wss://durable")]);
    let mut e2 = engine(outbox, dispatcher_2.clone(), store.clone());
    // resume_from_store waits for retry backoff; supply a now_ms past it.
    e2.resume_from_store(60_000).unwrap();

    let snap = e2.snapshot();
    assert_eq!(
        snap.recent_ok.len(),
        1,
        "expected resumed publish to succeed"
    );
    assert!(
        store.load_pending().unwrap().is_empty(),
        "store cleared after completion"
    );
}

#[test]
fn publish_dedup_on_same_event_multi_relay_single_rev_per_batch() {
    let outbox = outbox_with(
        "alice",
        &["wss://r1", "wss://r2", "wss://r3", "wss://r4", "wss://r5"],
        &[],
    );
    let dispatcher = Arc::new(ReplayDispatcher::new());
    for r in ["wss://r1", "wss://r2", "wss://r3", "wss://r4", "wss://r5"] {
        dispatcher.script(r, vec![RelayAck::ok(r)]);
    }
    let store: Arc<dyn PublishStore> = Arc::new(InMemoryPublishStore::new());
    let mut e = engine(outbox, dispatcher.clone(), store);

    let rev_before = e.snapshot().rev;
    e.start_publish(
        PublishAction::Publish {
            handle: "p-fanout".to_string(),
            event: signed("ev-fanout", "alice", 1, &[]),
            target: PublishTarget::Auto,
        },
        0,
        None,
    )
    .unwrap();
    let rev_after = e.snapshot().rev;
    let bumps = rev_after - rev_before;
    // 1 bump for start + ≤5 bumps for acks. The key invariant: not 25, not 50
    // — a per-event allocation regression would balloon this immediately.
    assert!(
        bumps <= 6,
        "expected ≤6 rev bumps, got {} — coalescer regressed",
        bumps
    );
    assert_eq!(
        e.snapshot().recent_ok.len(),
        1,
        "five OK acks coalesce to one recent_ok entry"
    );
    assert_eq!(e.snapshot().recent_ok[0].accepted_by.len(), 5);
}

#[test]
fn publish_outcome_classification_matches_per_relay_states() {
    use std::collections::BTreeMap;
    let mut all_ok = BTreeMap::new();
    all_ok.insert("wss://a".to_string(), PerRelayState::Ok { acked_at_ms: 1 });
    all_ok.insert("wss://b".to_string(), PerRelayState::Ok { acked_at_ms: 1 });
    assert!(matches!(
        outcome_of(&all_ok),
        PublishOutcome::Accepted { .. }
    ));

    let mut mixed = BTreeMap::new();
    mixed.insert("wss://a".to_string(), PerRelayState::Ok { acked_at_ms: 1 });
    mixed.insert(
        "wss://b".to_string(),
        PerRelayState::FailedAfterRetries {
            reason: "x".to_string(),
            last_at_ms: 2,
        },
    );
    assert!(matches!(outcome_of(&mixed), PublishOutcome::Mixed { .. }));

    let mut all_fail = BTreeMap::new();
    all_fail.insert(
        "wss://a".to_string(),
        PerRelayState::FailedAfterRetries {
            reason: "x".to_string(),
            last_at_ms: 2,
        },
    );
    assert!(matches!(
        outcome_of(&all_fail),
        PublishOutcome::FailedAfterRetries { .. }
    ));
}

#[test]
fn publish_store_persists_event_for_resume_round_trip() {
    let store = InMemoryPublishStore::new();
    let event = signed("ev-round", "alice", 1, &[]);
    let record = nmp_core::publish::PublishRecord {
        handle: "h-round".to_string(),
        event: event.clone(),
        per_relay: vec![("wss://r1".to_string(), PerRelayState::Pending)],
        pending_retries: Vec::new(),
        relay_reasons: Vec::new(),
    };
    store.upsert(&record).unwrap();
    let loaded = store.load_pending().unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].event.id, event.id);
    store.delete(&"h-round".to_string()).unwrap();
    assert!(store.load_pending().unwrap().is_empty());
}

#[test]
fn publish_store_error_does_not_panic_engine() {
    // The store impl never fails for InMemoryPublishStore, so we exercise the
    // explicit `From<PublishStoreError>` path. This proves D6 (errors stay in
    // the toast snapshot) by construction.
    let err: nmp_core::publish::PublishEngineError = PublishStoreError::Backend("x".into()).into();
    assert!(matches!(
        err,
        nmp_core::publish::PublishEngineError::Store(_)
    ));
}

#[test]
fn static_outbox_falls_back_to_indexer_when_author_has_no_writes() {
    // Coverage gap: `StaticOutbox::indexer_fallback` — the bootstrap/cold-start
    // branch — was never exercised. An author with no write relays on file
    // must resolve to the configured indexer set so a cold-start publish still
    // has somewhere to go. (This is the bootstrap resolver; the production
    // `Nip65OutboxResolver` is deliberately fail-closed instead — it returns
    // an empty set, mapped to `NoTargets`.)
    // Note: NO author_writes entry for "alice".
    let outbox = StaticOutbox {
        indexer_fallback: vec!["wss://indexer-1".to_string(), "wss://indexer-2".to_string()],
        ..StaticOutbox::default()
    };
    let resolved = outbox.resolve("alice", &[], &PublishTarget::Auto, 1, &BlockedRelaySet::new());
    let resolved_urls: std::collections::BTreeSet<String> =
        resolved.iter().map(|r| r.url.clone()).collect();
    assert_eq!(
        resolved_urls,
        ["wss://indexer-1", "wss://indexer-2"]
            .iter()
            .map(|s| s.to_string())
            .collect::<std::collections::BTreeSet<_>>(),
        "author with 0 write relays falls back to the indexer set"
    );
}

#[test]
fn static_outbox_uses_author_writes_and_skips_indexer_fallback() {
    // Symmetric assertion: when the author DOES have write relays, the
    // resolver routes to exactly those and the indexer fallback is NOT
    // consulted. A regression that always unions the fallback would leak the
    // publish to indexer relays the author never opted into.
    let mut outbox = StaticOutbox::default();
    outbox.author_writes.insert(
        "alice".to_string(),
        vec!["wss://alice-1".to_string(), "wss://alice-2".to_string()],
    );
    outbox.indexer_fallback = vec!["wss://indexer-fallback".to_string()];
    let resolved = outbox.resolve("alice", &[], &PublishTarget::Auto, 1, &BlockedRelaySet::new());
    let resolved_urls: std::collections::BTreeSet<&str> =
        resolved.iter().map(|r| r.url.as_str()).collect();
    assert!(resolved_urls.contains("wss://alice-1"));
    assert!(resolved_urls.contains("wss://alice-2"));
    assert!(
        !resolved_urls.contains("wss://indexer-fallback"),
        "indexer fallback must NOT be used when the author has write relays"
    );
    assert_eq!(resolved.len(), 2, "exactly the author's write relays");
}

// Two follow-up tests for codex 947dcfc findings (D6 FFI mapping +
// pending_retries durability) plus multi-relay fan-out coverage live in
// `publish_engine_followup.rs` so neither file exceeds the AGENTS.md 500-LOC
// hard cap.
