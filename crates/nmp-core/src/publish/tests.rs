//! Unit tests for the per-relay state machine, retry policy, and engine
//! orchestration. End-to-end integration tests (multi-relay, durability,
//! NIP-65 routing) live in `crates/nmp-core/tests/publish_engine.rs`.

use std::sync::Arc;

use super::action::{PublishAction, PublishTarget};
use super::engine::PublishEngine;
use super::state::{
    apply_ack, classify_ack, AckClass, PerRelayState, RelayAck, RetryPolicy, RetryVerdict,
};
use super::traits::{
    InMemoryPublishStore, NoopSigner, OutboxResolver, ReplayDispatcher, StaticOutbox,
};
use crate::substrate::{SignedEvent, UnsignedEvent};

fn signed_event(id: &str, author: &str, kind: u32, p_tags: &[&str]) -> SignedEvent {
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

fn engine_with(
    outbox: Arc<dyn OutboxResolver>,
    dispatcher: Arc<ReplayDispatcher>,
    policy: RetryPolicy,
) -> (
    PublishEngine,
    Arc<InMemoryPublishStore>,
    Arc<ReplayDispatcher>,
) {
    let store = Arc::new(InMemoryPublishStore::new());
    let signer = Arc::new(NoopSigner);
    let engine = PublishEngine::new(
        outbox,
        dispatcher.clone() as Arc<dyn super::traits::RelayDispatcher>,
        store.clone(),
        signer,
        policy,
    );
    (engine, store, dispatcher)
}

#[test]
fn state_machine_ok_settles_in_one_attempt() {
    let state = PerRelayState::InFlight {
        sent_at_ms: 1_000,
        attempt: 1,
    };
    let ack = RelayAck::ok("wss://r1");
    let verdict = apply_ack(&state, &ack, RetryPolicy::default(), 1_010);
    match verdict {
        RetryVerdict::Settled(PerRelayState::Ok { acked_at_ms }) => assert_eq!(acked_at_ms, 1_010),
        other => panic!("expected Ok settled, got {:?}", other),
    }
}

#[test]
fn state_machine_permanent_error_settles_failed() {
    let state = PerRelayState::InFlight {
        sent_at_ms: 1_000,
        attempt: 1,
    };
    let ack = RelayAck::failed("wss://r1", "blocked", "blocked: spam");
    let verdict = apply_ack(&state, &ack, RetryPolicy::default(), 1_010);
    assert!(matches!(
        verdict,
        RetryVerdict::Settled(PerRelayState::FailedAfterRetries { .. })
    ));
}

#[test]
fn state_machine_transient_retries_with_exponential_backoff() {
    let policy = RetryPolicy::default();
    let state = PerRelayState::InFlight {
        sent_at_ms: 1_000,
        attempt: 1,
    };
    let ack = RelayAck::failed("wss://r1", "io", "io");
    let verdict = apply_ack(&state, &ack, policy, 1_010);
    match verdict {
        RetryVerdict::ScheduleRetry {
            delay_ms,
            next_attempt,
        } => {
            assert_eq!(delay_ms, 1_000);
            assert_eq!(next_attempt, 2);
        }
        other => panic!("expected retry, got {:?}", other),
    }

    // Attempt 2 fails again
    let state = PerRelayState::InFlight {
        sent_at_ms: 2_010,
        attempt: 2,
    };
    let verdict = apply_ack(&state, &ack, policy, 2_020);
    match verdict {
        RetryVerdict::ScheduleRetry {
            delay_ms,
            next_attempt,
        } => {
            assert_eq!(delay_ms, 4_000);
            assert_eq!(next_attempt, 3);
        }
        other => panic!("expected retry, got {:?}", other),
    }

    // Attempt 3 fails — give up
    let state = PerRelayState::InFlight {
        sent_at_ms: 6_020,
        attempt: 3,
    };
    let verdict = apply_ack(&state, &ack, policy, 6_030);
    assert!(matches!(
        verdict,
        RetryVerdict::Settled(PerRelayState::FailedAfterRetries { .. })
    ));
}

#[test]
fn state_machine_auth_required_parks_without_consuming_retry_budget() {
    // Finding B: an `auth-required` OK reason must NOT consume a fast retry
    // budget (the challenge→sign→AUTH→OK round-trip — seconds, and slower for
    // bunker-signed AUTH — never completes inside one ≤250ms retry tick, so a
    // budget-consuming retry guarantees a false terminal failure). The pure
    // state machine signals a PARK: the engine demotes the relay to durable
    // Pending-awaiting-auth and re-dispatches only when the relay socket
    // reaches NIP-42 `Authenticated`. The attempt counter is preserved across
    // the park; no retry slot is spent.
    let policy = RetryPolicy::default();
    let ack = RelayAck::failed("wss://r1", "auth-required", "AUTH-REQUIRED: please AUTH");

    // Attempt 1 parks.
    let state = PerRelayState::InFlight {
        sent_at_ms: 1_000,
        attempt: 1,
    };
    assert!(
        matches!(apply_ack(&state, &ack, policy, 1_010), RetryVerdict::ParkAwaitingAuth { .. }),
        "first auth-required parks awaiting auth"
    );

    // An auth-required ack on a higher attempt count STILL parks — the park is
    // not budgeted, so repeated auth challenges can never settle the publish
    // terminally. (A relay that keeps demanding AUTH after we authenticate is a
    // relay/signer fault surfaced via the diagnostic auth-state lane, not a
    // publish FailedAfterRetries.)
    let state = PerRelayState::InFlight {
        sent_at_ms: 2_000,
        attempt: 5,
    };
    assert!(
        matches!(apply_ack(&state, &ack, policy, 2_010), RetryVerdict::ParkAwaitingAuth { .. }),
        "auth-required always parks; it never settles FailedAfterRetries by budget exhaustion"
    );
}

#[test]
fn state_machine_late_ack_for_terminal_is_idempotent() {
    let state = PerRelayState::Ok { acked_at_ms: 1_000 };
    let ack = RelayAck::failed("wss://r1", "io", "duplicate");
    let verdict = apply_ack(&state, &ack, RetryPolicy::default(), 2_000);
    assert!(matches!(
        verdict,
        RetryVerdict::Settled(PerRelayState::Ok { acked_at_ms: 1_000 })
    ));
}

#[test]
fn engine_explicit_target_dispatches_to_named_relays() {
    let outbox = Arc::new(StaticOutbox::default());
    let dispatcher = Arc::new(ReplayDispatcher::new());
    dispatcher.script("wss://r1", vec![RelayAck::ok("wss://r1")]);
    dispatcher.script("wss://r2", vec![RelayAck::ok("wss://r2")]);
    let (mut engine, _store, dispatcher) = engine_with(outbox, dispatcher, RetryPolicy::default());

    let action = PublishAction::Publish {
        handle: "h1".to_string(),
        event: signed_event("ev1", "alice", 1, &[]),
        target: PublishTarget::Explicit {
            relays: vec!["wss://r1".to_string(), "wss://r2".to_string()],
        },
    };
    engine.start_publish(action, 100, None).unwrap();

    let sent = dispatcher.sent_frames();
    let urls: Vec<String> = sent.iter().map(|(u, _)| u.clone()).collect();
    assert!(urls.contains(&"wss://r1".to_string()));
    assert!(urls.contains(&"wss://r2".to_string()));
    assert_eq!(sent.len(), 2);
    let snap = engine.snapshot();
    assert_eq!(snap.recent_ok.len(), 1);
    assert_eq!(snap.recent_ok[0].accepted_by.len(), 2);
}

#[test]
fn engine_auto_target_resolves_outbox_author_writes() {
    let mut outbox = StaticOutbox::default();
    outbox
        .author_writes
        .insert("alice".to_string(), vec!["wss://alice-write".to_string()]);
    let outbox = Arc::new(outbox);
    let dispatcher = Arc::new(ReplayDispatcher::new());
    dispatcher.script("wss://alice-write", vec![RelayAck::ok("wss://alice-write")]);
    let (mut engine, _store, dispatcher) = engine_with(outbox, dispatcher, RetryPolicy::default());

    let action = PublishAction::Publish {
        handle: "h2".to_string(),
        event: signed_event("ev2", "alice", 1, &[]),
        target: PublishTarget::Auto,
    };
    engine.start_publish(action, 100, None).unwrap();

    let urls: Vec<String> = dispatcher
        .sent_frames()
        .into_iter()
        .map(|(u, _)| u)
        .collect();
    assert_eq!(urls, vec!["wss://alice-write".to_string()]);
}

#[test]
fn engine_auto_target_includes_p_tag_inbox_relays() {
    let mut outbox = StaticOutbox::default();
    outbox
        .author_writes
        .insert("alice".to_string(), vec!["wss://alice-write".to_string()]);
    outbox
        .p_tag_reads
        .insert("bob".to_string(), vec!["wss://bob-read".to_string()]);
    let outbox = Arc::new(outbox);
    let dispatcher = Arc::new(ReplayDispatcher::new());
    dispatcher.script("wss://alice-write", vec![RelayAck::ok("wss://alice-write")]);
    dispatcher.script("wss://bob-read", vec![RelayAck::ok("wss://bob-read")]);
    let (mut engine, _store, dispatcher) = engine_with(outbox, dispatcher, RetryPolicy::default());

    let action = PublishAction::Publish {
        handle: "h3".to_string(),
        event: signed_event("ev3", "alice", 1, &["bob"]),
        target: PublishTarget::Auto,
    };
    engine.start_publish(action, 100, None).unwrap();

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
fn engine_no_targets_emits_recent_failure_and_errors() {
    let outbox = Arc::new(StaticOutbox::default());
    let dispatcher = Arc::new(ReplayDispatcher::new());
    let (mut engine, _store, _dispatcher) = engine_with(outbox, dispatcher, RetryPolicy::default());

    let action = PublishAction::Publish {
        handle: "h4".to_string(),
        event: signed_event("ev4", "alice", 1, &[]),
        target: PublishTarget::Auto,
    };
    let result = engine.start_publish(action, 100, None);
    assert!(matches!(
        result,
        Err(super::engine::PublishEngineError::NoTargets)
    ));
    assert_eq!(engine.snapshot().recent_errors.len(), 1);
    assert_eq!(
        engine.snapshot().recent_errors[0].reason,
        "no relays resolved for publish target"
    );
}

#[test]
fn engine_dedups_handle_on_double_start() {
    let mut outbox = StaticOutbox::default();
    outbox
        .author_writes
        .insert("alice".to_string(), vec!["wss://r1".to_string()]);
    let outbox = Arc::new(outbox);
    let dispatcher = Arc::new(ReplayDispatcher::new());
    // No ack scripted → publish stays InFlight; second start should reject.
    let (mut engine, _store, _dispatcher) = engine_with(outbox, dispatcher, RetryPolicy::default());

    let action = PublishAction::Publish {
        handle: "dup".to_string(),
        event: signed_event("ev5", "alice", 1, &[]),
        target: PublishTarget::Auto,
    };
    engine.start_publish(action.clone(), 100, None).unwrap();
    let dup = engine.start_publish(action, 200, None);
    assert!(matches!(
        dup,
        Err(super::engine::PublishEngineError::DuplicateHandle(_))
    ));
}

#[test]
fn engine_view_rev_bumps_once_per_batch_not_per_relay() {
    let mut outbox = StaticOutbox::default();
    outbox.author_writes.insert(
        "alice".to_string(),
        vec![
            "wss://r1".to_string(),
            "wss://r2".to_string(),
            "wss://r3".to_string(),
        ],
    );
    let outbox = Arc::new(outbox);
    let dispatcher = Arc::new(ReplayDispatcher::new());
    for r in ["wss://r1", "wss://r2", "wss://r3"] {
        dispatcher.script(r, vec![RelayAck::ok(r)]);
    }
    let (mut engine, _store, _dispatcher) = engine_with(outbox, dispatcher, RetryPolicy::default());

    let rev_before = engine.snapshot().rev;
    let action = PublishAction::Publish {
        handle: "fanout".to_string(),
        event: signed_event("ev6", "alice", 1, &[]),
        target: PublishTarget::Auto,
    };
    engine.start_publish(action, 100, None).unwrap();
    let rev_after = engine.snapshot().rev;
    // Rev should bump for the start (in-flight rows) plus once for each ack
    // settling, but the total is bounded — definitely far less than 3-per-relay
    // bursts and at most one bump per per-relay state transition (D8: at most
    // 60 Hz/view, and bursts batch). Empirically: start, plus 3 acks each
    // flipping a single per-relay slot. We assert tight bound so a regression
    // that re-introduces per-event rev churn fails loudly.
    let bumps = rev_after - rev_before;
    assert!(
        bumps <= 4,
        "expected at most 4 rev bumps (start + 3 acks), got {}",
        bumps
    );
    assert_eq!(engine.snapshot().recent_ok.len(), 1);
    assert_eq!(engine.snapshot().recent_ok[0].accepted_by.len(), 3);
}

#[test]
fn engine_exposes_classify_ack_to_actor_ffi_bridge() {
    // Per the task spec ("move classification to PublishEngine::classify_ack"),
    // the engine carries a method the actor / FFI bridge can call without
    // re-deriving the rules. The method routes to the pure `state::classify_ack`
    // so the policy stays single-sourced.
    let outbox = Arc::new(StaticOutbox::default());
    let dispatcher = Arc::new(ReplayDispatcher::new());
    let (engine, _store, _dispatcher) = engine_with(outbox, dispatcher, RetryPolicy::default());

    assert_eq!(
        engine.classify_ack(&RelayAck::failed("wss://r", "blocked", "no")),
        AckClass::Permanent
    );
    assert_eq!(
        engine.classify_ack(&RelayAck::failed("wss://r", "auth-required", "auth")),
        AckClass::AuthRequired
    );
    assert_eq!(
        engine.classify_ack(&RelayAck::timed_out("wss://r")),
        AckClass::Transient
    );
}

#[test]
fn classify_ack_maps_codes_to_engine_policy_d7_boundary() {
    // Regression guard: the dispatcher hands the engine purely descriptive
    // RelayAck values; classification into AckClass lives entirely inside
    // the engine (per D7 — capabilities report, never decide policy). If
    // anyone ever adds an `AckClass`-shaped field back to RelayAck this test
    // still passes, but the type system breaks first.

    // Permanent NIP-20 OK-false prefixes → Permanent.
    for code in [
        "blocked",
        "pow",
        "rate-limited",
        "restricted",
        "invalid",
        "duplicate",
        "mute",
    ] {
        let ack = RelayAck::failed("wss://r", code, "nope");
        assert_eq!(
            classify_ack(&ack),
            AckClass::Permanent,
            "code `{}` must map to Permanent",
            code
        );
    }

    // AUTH-REQUIRED → AuthRequired.
    let ack = RelayAck::failed("wss://r", "auth-required", "need auth");
    assert_eq!(classify_ack(&ack), AckClass::AuthRequired);

    // Transport-class + unknown codes → Transient (conservative retry).
    for code in ["timeout", "io", "connection-reset", "totally-new-code", ""] {
        let ack = RelayAck::failed("wss://r", code, "transport");
        assert_eq!(
            classify_ack(&ack),
            AckClass::Transient,
            "code `{}` must map to Transient (conservative default)",
            code
        );
    }

    // ok=true short-circuits to Permanent (success is permanent; the engine
    // never consults the classifier on success — pinning prevents accidents).
    let ack = RelayAck::ok("wss://r");
    assert_eq!(classify_ack(&ack), AckClass::Permanent);
}

#[test]
fn relay_ack_struct_carries_optional_details_for_dispatcher_metadata() {
    // RelayAck::details lets a dispatcher hand the engine richer transport
    // metadata (NIP-42 challenge, NIP-13 difficulty, retry-after-ms) without
    // baking a classification into the type. Per D7 the engine is free to
    // ignore it; classifiers only key off `code`.
    let mut ack = RelayAck::failed("wss://r", "auth-required", "please AUTH");
    ack.details = Some(serde_json::json!({"challenge": "deadbeef"}));

    // The classifier ignores `details` and routes solely on `code`.
    assert_eq!(classify_ack(&ack), AckClass::AuthRequired);

    // The struct round-trips through serde (FFI / persistence safety).
    let json = serde_json::to_string(&ack).unwrap();
    let back: RelayAck = serde_json::from_str(&json).unwrap();
    assert_eq!(back.details, ack.details);
    assert_eq!(back.code, "auth-required");
    assert!(!back.ok);

    // Ack with `ok=true` skips serialising `details` (defaults to None).
    let success = RelayAck::ok("wss://r");
    let json = serde_json::to_string(&success).unwrap();
    assert!(
        !json.contains("details"),
        "ok ack must not serialise None details (skip_serializing_if): {json}"
    );
}
