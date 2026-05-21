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
    InMemoryPublishStore, NoopSigner, QueueDispatcher, RelayDispatcher, ReplayDispatcher,
    StaticOutbox,
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
            None,
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
            None,
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

#[test]
fn correlation_id_override_is_reported_in_last_terminal_not_the_handle() {
    // THE FIX: a `PublishNote` dispatch mints a random correlation_id (the
    // event id is unknown — the actor signs the event). When the publish
    // settles, `last_terminal()` must report that minted id, NOT the publish
    // handle (== event id). Without the override the host's spinner — keyed
    // on the dispatch return value — could never be cleared.
    let mut outbox = StaticOutbox::default();
    outbox
        .author_writes
        .insert("alice".to_string(), vec!["wss://ok-a".to_string()]);
    let dispatcher = Arc::new(ReplayDispatcher::new());
    dispatcher.script("wss://ok-a", vec![RelayAck::ok("wss://ok-a")]);
    let mut engine = engine_with(Arc::new(outbox), dispatcher);

    // The minted action correlation_id (32-hex) differs from the event id.
    let minted_correlation_id = "ab".repeat(16);
    engine
        .start_publish(
            PublishAction::Publish {
                handle: "ev-publishnote".to_string(),
                event: signed_event("ev-publishnote", "alice", 1),
                target: PublishTarget::Auto,
            },
            100,
            Some(minted_correlation_id.clone()),
        )
        .unwrap();

    // The scripted OK settled the publish synchronously inside start_publish.
    let terminal = engine
        .last_terminal()
        .expect("a settled publish must record a LastTerminal");
    assert_eq!(
        terminal.correlation_id, minted_correlation_id,
        "last_terminal must report the minted correlation_id, not the handle"
    );
    assert_ne!(
        terminal.correlation_id, "ev-publishnote",
        "the publish handle (event id) must NOT leak as the correlation_id"
    );
    assert_eq!(terminal.status, "ok", "the all-OK publish settles ok");
}

#[test]
fn no_correlation_id_override_falls_back_to_handle_in_last_terminal() {
    // The pre-existing behaviour for every non-dispatch publish path
    // (`react`, `follow`, pre-signed `Publish`): with no override, the
    // terminal verdict reports the publish handle (== event id). This guards
    // against the fix accidentally changing the handle-as-correlation-id
    // contract the publish-queue tests depend on.
    let mut outbox = StaticOutbox::default();
    outbox
        .author_writes
        .insert("alice".to_string(), vec!["wss://ok-a".to_string()]);
    let dispatcher = Arc::new(ReplayDispatcher::new());
    dispatcher.script("wss://ok-a", vec![RelayAck::ok("wss://ok-a")]);
    let mut engine = engine_with(Arc::new(outbox), dispatcher);

    engine
        .start_publish(
            PublishAction::Publish {
                handle: "ev-presigned".to_string(),
                event: signed_event("ev-presigned", "alice", 1),
                target: PublishTarget::Auto,
            },
            100,
            None,
        )
        .unwrap();

    let terminal = engine
        .last_terminal()
        .expect("a settled publish must record a LastTerminal");
    assert_eq!(
        terminal.correlation_id, "ev-presigned",
        "with no override the terminal verdict reports the handle (event id)"
    );
}

#[test]
fn inflight_timeout_sweep_transitions_stuck_relay_through_retry_to_failure() {
    // Regression guard for the critical bug: a relay that accepts the socket
    // but never sends `OK` (and never closes) pinned the publish in `InFlight`
    // forever because `tick` never examined `sent_at_ms`.
    //
    // Scenario: QueueDispatcher returns no acks (simulates silent drop). After
    // `inflight_deadline_ms` elapses the sweeper must transition the relay to
    // `TimedOut`; the retry ladder eventually settles it to `FailedAfterRetries`,
    // producing a `RecentFailure` row and a `TerminalOutcome` for the kernel.
    let mut outbox = StaticOutbox::default();
    outbox
        .author_writes
        .insert("alice".to_string(), vec!["wss://silent".to_string()]);
    // QueueDispatcher → dispatch() returns Vec::new() (no synchronous ack),
    // simulating a relay that accepts the socket but never sends OK or closes.
    let dispatcher = Arc::new(QueueDispatcher::new());
    let policy = RetryPolicy {
        transient_max_retries: 2,  // attempt 1 → timeout → attempt 2 → timeout → fail
        inflight_deadline_ms: 5_000,
        backoff_base_ms: 0,        // no backoff so ticks are predictable
        ..RetryPolicy::default()
    };
    let mut engine = PublishEngine::new(
        Arc::new(outbox),
        dispatcher.clone() as Arc<dyn RelayDispatcher>,
        Arc::new(InMemoryPublishStore::new()),
        Arc::new(NoopSigner),
        policy,
    );

    let t0: u64 = 1_000_000;
    engine
        .start_publish(
            PublishAction::Publish {
                handle: "h1".to_string(),
                event: signed_event("ev-timeout", "alice", 1),
                target: PublishTarget::Auto,
            },
            t0,
            None,
        )
        .unwrap();

    // Before the deadline: relay stays InFlight, no completed outcomes.
    engine.tick(t0 + 4_000);
    assert!(
        engine.take_completed().is_empty(),
        "relay should still be InFlight before the deadline"
    );

    // First deadline: sweeper fires. Attempt 1 < transient_max_retries (2) →
    // transitions to TimedOut and is immediately re-dispatched as attempt 2.
    engine.tick(t0 + 5_000);
    assert!(
        engine.take_completed().is_empty(),
        "relay should be retried (attempt 2), not yet failed"
    );

    // Second deadline: attempt 2 >= transient_max_retries (2) →
    // sweep transitions directly to FailedAfterRetries → publish settles.
    engine.tick(t0 + 10_001);
    let completed = engine.take_completed();
    assert_eq!(
        completed.len(),
        1,
        "publish must settle to FailedAfterRetries after retries exhausted"
    );
    assert!(
        completed[0].failed.iter().any(|(url, _)| url == "wss://silent"),
        "the silent relay must appear in the failed list"
    );
    assert!(
        completed[0].accepted.is_empty(),
        "no relay accepted the event"
    );
}
