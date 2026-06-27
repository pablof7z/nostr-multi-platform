//! Integration tests for the `ExternalEventSinkDispatcher` — the in-process
//! relay-forwarding path (bind_runtime → dispatch → relay policy), duplicate
//! fan-out, and policy-panic isolation.
//!
//! Split out of `dispatcher.rs` to honor the AGENTS.md 500-line file ceiling.
//! `super` resolves to the `dispatcher` module (included via `#[path]`).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::ExternalEventSinkDispatcher;
use crate::actor::KindFilter;
use crate::store::RawEvent;
use crate::substrate::external_event_sink::{
    ExternalEventSinkPolicy, IngestOutcomeKind, SignedEventFrame, SinkDestination,
};

fn make_pool() -> nmp_network::pool::Pool {
    let (relay_tx, _relay_rx) = std::sync::mpsc::channel();
    nmp_network::pool::Pool::new(nmp_network::pool::PoolConfig::default(), relay_tx)
}

fn make_raw(kind: u32, created_at: u64, id_byte: &str) -> RawEvent {
    RawEvent {
        id: id_byte.repeat(32),
        pubkey: "cd".repeat(32),
        created_at,
        kind,
        tags: Vec::new(),
        content: String::new(),
        sig: "ef".repeat(64),
    }
}

fn dispatch_one(
    d: &ExternalEventSinkDispatcher,
    kind: u32,
    created_at: u64,
    id_byte: &str,
    outcome: IngestOutcomeKind,
) -> bool {
    let raw = Arc::new(make_raw(kind, created_at, id_byte));
    let frame = SignedEventFrame::build(raw, None, outcome).expect("build frame");
    d.dispatch(frame)
}

fn wait_until<F: Fn() -> bool>(pred: F) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if pred() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    pred()
}

// ─── policy capture (relay path) ──────────────────────────────────────────

#[derive(Clone, Default)]
struct CapturePolicy {
    frames: Arc<Mutex<Vec<SignedEventFrame>>>,
}
impl CapturePolicy {
    fn frames(&self) -> Vec<SignedEventFrame> {
        self.frames.lock().expect("frames lock").clone()
    }
}
impl ExternalEventSinkPolicy for CapturePolicy {
    fn kind_filter(&self) -> KindFilter {
        KindFilter::from_kinds([1u32])
    }
    fn destinations(&self, frame: &SignedEventFrame) -> Vec<SinkDestination> {
        self.frames.lock().expect("frames lock").push(frame.clone());
        Vec::new()
    }
}

/// A policy that always panics in `destinations`.
#[derive(Clone, Default)]
struct PanicPolicy;
impl ExternalEventSinkPolicy for PanicPolicy {
    fn kind_filter(&self) -> KindFilter {
        KindFilter::from_kinds([1u32])
    }
    fn destinations(&self, _frame: &SignedEventFrame) -> Vec<SinkDestination> {
        panic!("policy panic (test): must NOT kill the worker");
    }
}

// ─── tests ────────────────────────────────────────────────────────────────

/// CRITICAL INVARIANT (design §a): a `Duplicate` outcome MUST reach the
/// policy with source-relay provenance. Kept passing across the refactor.
#[test]
fn duplicate_outcome_reaches_sink() {
    let d = ExternalEventSinkDispatcher::new();
    d.bind_runtime(make_pool());
    let capture = Arc::new(CapturePolicy::default());
    d.set_policies(vec![capture.clone() as Arc<dyn ExternalEventSinkPolicy>]);

    let raw = Arc::new(make_raw(1, 1_700_000_000, "ab"));
    let frame = SignedEventFrame::build(
        raw,
        Some(Arc::from("wss://second-relay.example/")),
        IngestOutcomeKind::Duplicate,
    )
    .expect("build SignedEventFrame for Duplicate outcome");
    assert!(
        d.dispatch(frame),
        "dispatch must enqueue when a policy matches"
    );

    assert!(
        wait_until(|| capture.frames().len() == 1),
        "exactly one Duplicate frame must reach the policy"
    );
    let frames = capture.frames();
    let f = &frames[0];
    assert_eq!(f.ingest_outcome, IngestOutcomeKind::Duplicate);
    assert_eq!(
        f.source_relay.as_deref(),
        Some("wss://second-relay.example/")
    );
    assert_eq!(f.raw.kind, 1);
}

/// A policy that returns an empty (match-all) KindFilter is silently dropped
/// at registration time (#1607 — all-kind raw-tap policies are banned).
#[test]
fn all_kind_filter_policy_is_rejected_at_registration() {
    struct AllKindsPolicy;
    impl ExternalEventSinkPolicy for AllKindsPolicy {
        fn kind_filter(&self) -> KindFilter {
            KindFilter::default() // empty = match all — banned
        }
        fn destinations(&self, _frame: &SignedEventFrame) -> Vec<SinkDestination> {
            unreachable!("all-kind policy must never be dispatched")
        }
    }

    let d = ExternalEventSinkDispatcher::new();
    d.bind_runtime(make_pool());
    d.set_policies(vec![
        Arc::new(AllKindsPolicy) as Arc<dyn ExternalEventSinkPolicy>
    ]);

    // The policy should have been rejected: dispatching any kind returns false.
    let dispatched = dispatch_one(&d, 1, 200, "cc", IngestOutcomeKind::Inserted);
    assert!(
        !dispatched,
        "rejected all-kind policy must not receive any frame"
    );
}

/// A panicking policy does not kill the worker: a healthy capture policy on
/// the SAME frame still receives it, and a later frame is still delivered.
#[test]
fn panicking_policy_does_not_kill_worker() {
    let d = ExternalEventSinkDispatcher::new();
    d.bind_runtime(make_pool());
    let capture = Arc::new(CapturePolicy::default());
    d.set_policies(vec![
        Arc::new(PanicPolicy) as Arc<dyn ExternalEventSinkPolicy>,
        capture.clone() as Arc<dyn ExternalEventSinkPolicy>,
    ]);

    assert!(dispatch_one(&d, 1, 100, "1a", IngestOutcomeKind::Inserted));
    assert!(dispatch_one(&d, 1, 101, "2a", IngestOutcomeKind::Inserted));
    assert!(
        wait_until(|| capture.frames().len() == 2),
        "healthy policy must still receive both frames despite the panicking sibling"
    );
    assert!(
        wait_until(|| d.diagnostics().policy_panics >= 2),
        "policy panics must be counted, not fatal"
    );
}
