//! T146 — kernel event observer fan-out at the ingest seam.
//!
//! Drives synthetic kind:1 events through `ingest_pre_verified_event` (the
//! test-support ingest path used by every internal test) and asserts that a
//! Rust trait-object observer attached to the kernel's slot fires exactly
//! once per accepted event.
//!
//! The fan-out path is shared with production: `ingest/timeline.rs` makes
//! the same `notify_event_observers(&kernel_event)` call after each
//! `EventStore::insert` returning `Inserted | Replaced`. See ADR-0009 (D0 —
//! kernel emits, per-app crates compose) for the architectural rationale.

use super::*;
use crate::actor::{new_event_observer_slot, register_rust_observer, KernelEventObserver};
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};
use crate::store::{RawEvent, VerifiedEvent};
use crate::substrate::KernelEvent;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

struct CapturingObserver {
    count: AtomicU32,
    last: Mutex<Option<KernelEvent>>,
}

impl CapturingObserver {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            count: AtomicU32::new(0),
            last: Mutex::new(None),
        })
    }
}

impl KernelEventObserver for CapturingObserver {
    fn on_kernel_event(&self, event: &KernelEvent) {
        self.count.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut guard) = self.last.lock() {
            *guard = Some(event.clone());
        }
    }
}

fn raw(id: &str, kind: u32) -> RawEvent {
    RawEvent {
        id: id.to_string(),
        pubkey: "a".repeat(64),
        created_at: 1_700_000_000,
        kind,
        tags: vec![],
        content: "hi".to_string(),
        sig: "a".repeat(128),
    }
}

#[test]
fn observer_fires_once_per_kind1_ingest() {
    let slot = new_event_observer_slot();
    let observer = CapturingObserver::new();
    register_rust_observer(&slot, observer.clone());

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_event_observers_handle(slot);

    for i in 0..5u32 {
        // Distinct ids per event so the store treats each as Inserted (not
        // a duplicate of a previously-stored event).
        let id = format!("{:064x}", i);
        kernel.ingest_pre_verified_event(
            RelayRole::Content,
            "diag-firehose-stress",
            VerifiedEvent::from_raw_unchecked(raw(&id, 1)),
        );
    }

    assert_eq!(
        observer.count.load(Ordering::SeqCst),
        5,
        "observer must fire exactly once per accepted ingest"
    );
}

#[test]
fn observer_receives_event_with_correct_fields() {
    let slot = new_event_observer_slot();
    let observer = CapturingObserver::new();
    register_rust_observer(&slot, observer.clone());

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_event_observers_handle(slot);

    let id = "b".repeat(64);
    kernel.ingest_pre_verified_event(
        RelayRole::Content,
        "diag-firehose-stress",
        VerifiedEvent::from_raw_unchecked(raw(&id, 1)),
    );

    let captured = observer
        .last
        .lock()
        .unwrap()
        .clone()
        .expect("observer fired");
    assert_eq!(captured.id, id);
    assert_eq!(captured.kind, 1);
    assert_eq!(captured.author, "a".repeat(64));
    assert_eq!(captured.content, "hi");
}

#[test]
fn no_observer_handle_is_silent_noop() {
    // Kernel without any observer slot bound (the default for `Kernel::new`).
    // Ingest still works; just no fan-out. Sanity check that the fan-out
    // branch handles the `None` slot.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let id = "c".repeat(64);
    kernel.ingest_pre_verified_event(
        RelayRole::Content,
        "diag-firehose-stress",
        VerifiedEvent::from_raw_unchecked(raw(&id, 1)),
    );
    assert!(kernel.events.contains_key(&id), "event still ingested");
}

#[test]
fn duplicate_ingest_does_not_double_fire() {
    let slot = new_event_observer_slot();
    let observer = CapturingObserver::new();
    register_rust_observer(&slot, observer.clone());

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_event_observers_handle(slot);

    let id = "d".repeat(64);
    let r = raw(&id, 1);
    kernel.ingest_pre_verified_event(
        RelayRole::Content,
        "diag-firehose-stress",
        VerifiedEvent::from_raw_unchecked(r.clone()),
    );
    kernel.ingest_pre_verified_event(
        RelayRole::Content,
        "diag-firehose-stress",
        VerifiedEvent::from_raw_unchecked(r),
    );

    // Two ingests of the same id — store returns Duplicate the second
    // time, which short-circuits before `notify_event_observers`. Exactly
    // one observer call.
    assert_eq!(
        observer.count.load(Ordering::SeqCst),
        1,
        "duplicate ingest must not fire observer twice"
    );
}
