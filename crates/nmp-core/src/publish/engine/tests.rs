//! Engine-internal tests for the T128 terminal-outcome drain
//! (`PublishEngine::take_completed` / `TerminalOutcome`).
//!
//! These live in-crate (not in `tests/`) because `take_completed` is
//! `pub(crate)` — it is the kernel's projection hook, not a public API. The
//! kernel calls it after every engine entrypoint to flip its
//! `PublishQueueEntry` projection from `accepted_locally` to `"ok"` /
//! `"failed"`. The state-machine and basic-orchestration tests stay in
//! `publish/tests.rs`; this file isolates the terminal-drain concern.

use std::sync::Arc;

use super::PublishEngine;
use crate::publish::action::{PublishAction, PublishTarget};
use crate::publish::state::{RelayAck, RetryPolicy};
use crate::publish::traits::{
    InMemoryPublishStore, NoopSigner, RelayDispatcher, ReplayDispatcher, StaticOutbox,
};
use crate::substrate::{SignedEvent, UnsignedEvent};

fn signed_event(id: &str, author: &str, kind: u32) -> SignedEvent {
    SignedEvent {
        id: id.to_string(),
        sig: format!("sig-{}", id),
        unsigned: UnsignedEvent {
            pubkey: author.to_string(),
            kind,
            tags: Vec::new(),
            content: format!("content-{}", id),
            created_at: 1_700_000_000,
        },
    }
}

fn engine_with(outbox: Arc<StaticOutbox>, dispatcher: Arc<ReplayDispatcher>) -> PublishEngine {
    PublishEngine::new(
        outbox,
        dispatcher as Arc<dyn RelayDispatcher>,
        Arc::new(InMemoryPublishStore::new()),
        Arc::new(NoopSigner),
        RetryPolicy::default(),
    )
}

#[test]
fn engine_take_completed_drains_terminal_outcome_then_empties() {
    // `take_completed` is the kernel's projection hook — it drains the
    // per-handle `TerminalOutcome` recorded the moment a publish settles,
    // before the in-flight row is evicted. The kernel relies on: (1) exactly
    // one outcome per settled handle, (2) the accepted/failed split is
    // correct, (3) a second drain yields nothing (pure drain — no replay).
    let mut outbox = StaticOutbox::default();
    outbox.author_writes.insert(
        "alice".to_string(),
        vec!["wss://ok-a".to_string(), "wss://ok-b".to_string()],
    );
    let dispatcher = Arc::new(ReplayDispatcher::new());
    dispatcher.script("wss://ok-a", vec![RelayAck::ok("wss://ok-a")]);
    dispatcher.script("wss://ok-b", vec![RelayAck::ok("wss://ok-b")]);
    let mut engine = engine_with(Arc::new(outbox), dispatcher);

    engine
        .start_publish(
            PublishAction::Publish {
                handle: "tc1".to_string(),
                event: signed_event("ev-tc1", "alice", 1),
                target: PublishTarget::Auto,
            },
            100,
        )
        .unwrap();

    // The publish settled inside start_publish (both acks scripted OK). The
    // engine must have recorded exactly one terminal outcome for the handle.
    let drained = engine.take_completed();
    assert_eq!(drained.len(), 1, "one settled handle → one TerminalOutcome");
    let outcome = &drained[0];
    assert_eq!(outcome.event_id, "ev-tc1");
    let mut accepted = outcome.accepted.clone();
    accepted.sort();
    assert_eq!(
        accepted,
        vec!["wss://ok-a".to_string(), "wss://ok-b".to_string()],
        "both relays land in the accepted list"
    );
    assert!(
        outcome.failed.is_empty(),
        "no failures on an all-OK publish"
    );

    // Pure drain: a second call yields nothing — the engine keeps no
    // per-publish history after the kernel has consumed it.
    assert!(
        engine.take_completed().is_empty(),
        "take_completed is a pure drain — second call is empty"
    );
}

#[test]
fn engine_take_completed_reports_mixed_accepted_and_failed_split() {
    // A mixed publish (≥1 Ok + ≥1 permanent failure) must surface BOTH lists
    // on the same `TerminalOutcome` so the kernel can decide what status
    // string to project. This is the kernel-facing twin of the snapshot's
    // recent_ok / recent_errors rings.
    let mut outbox = StaticOutbox::default();
    outbox.author_writes.insert(
        "alice".to_string(),
        vec!["wss://good".to_string(), "wss://bad".to_string()],
    );
    let dispatcher = Arc::new(ReplayDispatcher::new());
    dispatcher.script("wss://good", vec![RelayAck::ok("wss://good")]);
    // "blocked" is a permanent NIP-20 code → settles FailedAfterRetries with
    // no retries, so the publish completes in one batch.
    dispatcher.script(
        "wss://bad",
        vec![RelayAck::failed("wss://bad", "blocked", "blocked: spam")],
    );
    let mut engine = engine_with(Arc::new(outbox), dispatcher);

    engine
        .start_publish(
            PublishAction::Publish {
                handle: "tc-mix".to_string(),
                event: signed_event("ev-tc-mix", "alice", 1),
                target: PublishTarget::Auto,
            },
            100,
        )
        .unwrap();

    let drained = engine.take_completed();
    assert_eq!(drained.len(), 1);
    let outcome = &drained[0];
    assert_eq!(
        outcome.accepted,
        vec!["wss://good".to_string()],
        "the accepting relay is in `accepted`"
    );
    assert_eq!(
        outcome.failed.len(),
        1,
        "the rejecting relay is in `failed`"
    );
    assert_eq!(outcome.failed[0].0, "wss://bad");
    assert!(
        outcome.failed[0].1.contains("blocked"),
        "failure reason is carried for the kernel: {:?}",
        outcome.failed[0].1
    );
}
