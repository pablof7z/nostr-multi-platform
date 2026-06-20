//! Tests for ADR-0062 — observer-scoped read-model catch-up.
//!
//! Each test injects events into the kernel read-cache via the existing
//! `ingest_pre_verified_event` test-support path (which populates
//! `self.events` and fires the global fan-out), THEN registers a muted
//! observer and calls `open_interest_with_observer_replay`, and asserts
//! that matching events replay to that observer only.

use super::*;
use crate::actor::{
    new_event_observer_slot, register_rust_observer, register_rust_observer_muted,
    KernelEventObserver,
};
use crate::kernel::observer_replay::ObserverReplayRequest;
use crate::planner::{InterestShape, LogicalInterest};
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};
use crate::store::{RawEvent, VerifiedEvent};
use crate::substrate::KernelEvent;
use crate::subs::{SubIdentity, SubKey};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

// ─── Test helpers ─────────────────────────────────────────────────────────────

struct CapturingObserver {
    events: Mutex<Vec<KernelEvent>>,
}

impl CapturingObserver {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            events: Mutex::new(Vec::new()),
        })
    }

    fn ids(&self) -> Vec<String> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.id.clone())
            .collect()
    }

    fn count(&self) -> usize {
        self.events.lock().unwrap().len()
    }
}

impl KernelEventObserver for CapturingObserver {
    fn on_kernel_event(&self, event: &KernelEvent) {
        self.events.lock().unwrap().push(event.clone());
    }
}

/// Ingest a minimal kind:1 event into the kernel and return the event id.
fn ingest(kernel: &mut Kernel, id: &str, author: &str, created_at: u64, tags: Vec<Vec<String>>) {
    let raw = RawEvent {
        id: id.to_string(),
        pubkey: author.to_string(),
        created_at,
        kind: 1,
        tags,
        content: "test".into(),
        sig: "a".repeat(128),
    };
    kernel.ingest_pre_verified_event(
        RelayRole::Content,
        "test-relay",
        VerifiedEvent::from_raw_unchecked(raw),
    );
}

/// Build a simple author+kinds interest shape.
fn author_shape(author: &str, kinds: &[u32]) -> InterestShape {
    let k = kinds.iter().map(|k| k.to_string()).collect::<Vec<_>>().join(",");
    InterestShape::from_filter_json(&format!(r#"{{"kinds":[{k}],"authors":["{author}"]}}"#))
        .expect("valid author shape")
}

/// Build a SubIdentity for testing (filter_json + consumer_id + scope).
fn sub_identity(filter_json: &str, consumer_id: &str, scope: u32) -> SubIdentity {
    crate::subs::interest_builder::build_interest_pair(filter_json, consumer_id, scope)
        .map(|(id, _)| id)
        .expect("valid filter → identity")
}

/// Build a LogicalInterest (Tailing) for testing.
fn logical_interest(filter_json: &str, consumer_id: &str, scope: u32) -> LogicalInterest {
    crate::subs::interest_builder::build_interest_pair(filter_json, consumer_id, scope)
        .map(|(_, interest)| interest)
        .expect("valid filter → interest")
}

fn author_filter_json(author: &str) -> String {
    format!(r#"{{"kinds":[1],"authors":["{author}"]}}"#)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

/// Core correctness: inject events THEN open feed — the muted observer must
/// receive the cached events via replay and no more.
#[test]
fn replay_delivers_cached_events_to_muted_observer_only() {
    let slot = new_event_observer_slot();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_event_observers_handle(slot.clone());

    let author = "a".repeat(64);
    ingest(&mut kernel, &"01".repeat(32), &author, 1_000, vec![]);
    ingest(&mut kernel, &"02".repeat(32), &author, 2_000, vec![]);
    // Event from a different author — must NOT replay.
    let other = "b".repeat(64);
    ingest(&mut kernel, &"03".repeat(32), &other, 3_000, vec![]);

    // Register an already-active bystander to verify global fan-out doesn't
    // double-deliver during replay.
    let bystander = CapturingObserver::new();
    register_rust_observer(&slot, bystander.clone());

    // Now open observed interest for author.
    let capturing = CapturingObserver::new();
    let observer_id = register_rust_observer_muted(&slot, capturing.clone());

    let filter_json = author_filter_json(&author);
    let identity = sub_identity(&filter_json, "test-consumer", 1);
    let interest = logical_interest(&filter_json, "test-consumer", 1);
    let replay = ObserverReplayRequest {
        observer_id,
        shapes: vec![author_shape(&author, &[1])],
        limit: 80,
    };
    kernel.open_interest_with_observer_replay(identity, interest, replay, "test");

    // Author events must have replayed to the capturing observer.
    let ids = capturing.ids();
    assert_eq!(ids.len(), 2, "capturing observer must receive exactly the 2 author events");
    assert!(ids.contains(&"01".repeat(32)), "event 01 replayed");
    assert!(ids.contains(&"02".repeat(32)), "event 02 replayed");

    // Bystander must NOT have received any event from the replay step
    // (replay uses targeted delivery, not global fan-out).
    assert_eq!(
        bystander.count(),
        0,
        "bystander must not receive events from targeted replay"
    );
}

/// After activation, a subsequent global notify reaches the observer.
#[test]
fn observer_fires_on_global_notify_after_activation() {
    let slot = new_event_observer_slot();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_event_observers_handle(slot.clone());

    let author = "c".repeat(64);
    ingest(&mut kernel, &"a0".repeat(32), &author, 1_000, vec![]);

    let capturing = CapturingObserver::new();
    let observer_id = register_rust_observer_muted(&slot, capturing.clone());

    let filter_json = author_filter_json(&author);
    let identity = sub_identity(&filter_json, "test-consumer-2", 1);
    let interest = logical_interest(&filter_json, "test-consumer-2", 1);
    let replay = ObserverReplayRequest {
        observer_id,
        shapes: vec![author_shape(&author, &[1])],
        limit: 80,
    };
    kernel.open_interest_with_observer_replay(identity, interest, replay, "test");

    // Replay delivered one event.
    assert_eq!(capturing.count(), 1, "replay delivered 1 event");

    // Inject a NEW event (arrives AFTER open_interest_with_observer_replay).
    // This fires the GLOBAL fan-out, which now includes our capturing observer
    // because activate_observer was called inside open_interest_with_observer_replay.
    ingest(&mut kernel, &"b0".repeat(32), &author, 2_000, vec![]);
    assert_eq!(capturing.count(), 2, "newly ingested event reaches activated observer");
}

/// Multi-owner scenario: two observers for the same author shape.
/// The second one (changed:false from EnsureAbsent) still replays.
#[test]
fn second_observer_replays_despite_changed_false() {
    let slot = new_event_observer_slot();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_event_observers_handle(slot.clone());

    let author = "d".repeat(64);
    ingest(&mut kernel, &"c0".repeat(32), &author, 1_000, vec![]);
    ingest(&mut kernel, &"c1".repeat(32), &author, 2_000, vec![]);

    let filter_json = author_filter_json(&author);

    // First observer.
    let obs1 = CapturingObserver::new();
    let id1 = register_rust_observer_muted(&slot, obs1.clone());
    let identity1 = sub_identity(&filter_json, "test-consumer-3a", 1);
    let interest1 = logical_interest(&filter_json, "test-consumer-3a", 1);
    let replay1 = ObserverReplayRequest {
        observer_id: id1,
        shapes: vec![author_shape(&author, &[1])],
        limit: 80,
    };
    let newly_installed = kernel.open_interest_with_observer_replay(identity1, interest1, replay1, "test");
    assert!(newly_installed, "first open should newly install");

    // Second observer for the SAME shape (different consumer_id → different SubIdentity,
    // but same SubKey hash → EnsureAbsent returns changed:false on second open).
    let obs2 = CapturingObserver::new();
    let id2 = register_rust_observer_muted(&slot, obs2.clone());
    let identity2 = sub_identity(&filter_json, "test-consumer-3b", 1);
    let interest2 = logical_interest(&filter_json, "test-consumer-3b", 1);
    let replay2 = ObserverReplayRequest {
        observer_id: id2,
        shapes: vec![author_shape(&author, &[1])],
        limit: 80,
    };
    kernel.open_interest_with_observer_replay(identity2, interest2, replay2, "test");

    // Both observers must have received the 2 events regardless.
    assert_eq!(obs1.count(), 2, "first observer replayed 2 events");
    assert_eq!(obs2.count(), 2, "second observer replayed 2 events despite changed:false");
}

/// No double-delivery: an event replayed during catch-up must NOT re-arrive
/// via global fan-out (the global fan-out only fires for events ingested AFTER
/// `open_interest_with_observer_replay`).
#[test]
fn no_double_delivery_on_replay_then_live() {
    let slot = new_event_observer_slot();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_event_observers_handle(slot.clone());

    let author = "e".repeat(64);
    // Inject BEFORE open — this ends up in the read-cache and will be replayed.
    ingest(&mut kernel, &"d0".repeat(32), &author, 1_000, vec![]);

    let obs = CapturingObserver::new();
    let observer_id = register_rust_observer_muted(&slot, obs.clone());

    let filter_json = author_filter_json(&author);
    let identity = sub_identity(&filter_json, "test-consumer-4", 1);
    let interest = logical_interest(&filter_json, "test-consumer-4", 1);
    let replay = ObserverReplayRequest {
        observer_id,
        shapes: vec![author_shape(&author, &[1])],
        limit: 80,
    };
    kernel.open_interest_with_observer_replay(identity, interest, replay, "test");

    // Replay delivered 1.
    assert_eq!(obs.count(), 1, "replay delivered 1 event");

    // A DIFFERENT event arrives live AFTER open — should fire once via global fan-out.
    ingest(&mut kernel, &"d1".repeat(32), &author, 2_000, vec![]);
    assert_eq!(obs.count(), 2, "live event arrives exactly once via global fan-out");

    // The d0 event does NOT re-arrive (global fan-out calls notify_event_observers
    // for the new d1 event only, not d0).
    let ids = obs.ids();
    let d0_occurrences = ids.iter().filter(|&id| id == &"d0".repeat(32)).count();
    assert_eq!(d0_occurrences, 1, "replayed event d0 must appear exactly once total");
}

/// D9 clock clamp: future-dated events in the read-cache must have their
/// `created_at` clamped to `now_secs()` during replay.
#[test]
fn replay_clamps_future_dated_events_to_now() {
    let slot = new_event_observer_slot();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_event_observers_handle(slot.clone());

    let author = "f".repeat(64);
    // Inject a far-future event (year 2200 ≈ ts 7_258_118_400).
    let far_future = 7_258_118_400u64;
    ingest(&mut kernel, &"e0".repeat(32), &author, far_future, vec![]);

    let obs = CapturingObserver::new();
    let observer_id = register_rust_observer_muted(&slot, obs.clone());

    let filter_json = author_filter_json(&author);
    let identity = sub_identity(&filter_json, "test-consumer-5", 1);
    let interest = logical_interest(&filter_json, "test-consumer-5", 1);
    let replay = ObserverReplayRequest {
        observer_id,
        shapes: vec![author_shape(&author, &[1])],
        limit: 80,
    };
    kernel.open_interest_with_observer_replay(identity, interest, replay, "test");

    assert_eq!(obs.count(), 1, "future-dated event was replayed");
    let ev = obs.events.lock().unwrap()[0].clone();
    let now = kernel.now_secs();
    assert!(
        ev.created_at <= now,
        "replayed created_at ({}) must be clamped to now ({})",
        ev.created_at,
        now
    );
}
