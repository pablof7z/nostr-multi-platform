//! Observed projection sink slot.
//!
//! This is internal kernel plumbing for declared observed projections. A
//! production app/product read model must declare its event shape, owner,
//! scope, and replay bounds before it receives events; it must not register a
//! filterless all-event observer.
//!
//! The slot supports only:
//!
//! - muted sink registration;
//! - targeted replay by sink id;
//! - activation into future delivery scoped to declared `InterestShape`s.
//!
//! ## Doctrine
//!
//! * **D0/D5/D8** — no public ambient accepted-event stream. Read models are
//!   declared resources and future delivery is shape-scoped.
//! * **D6** — a poisoned mutex or panicking sink is a silent no-op.
//! * **Re-entrancy** — invocation snapshots the registration list under the
//!   lock, then releases the lock before invoking sinks.

mod delivery;

pub use delivery::activate_observer_scoped;

use crate::substrate::KernelEvent;
use delivery::RustObserverDelivery;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex};

/// Stable id returned by scoped registration so callers can close the same
/// observed-projection session later.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct ObservedProjectionId(pub u64);

/// One sink registration entry.
///
/// Muted registrations are reachable by `notify_observer_by_id` for targeted
/// replay and are promoted by `activate_observer_scoped` once catch-up has
/// completed.
pub struct RustObserverRegistration {
    pub(super) id: ObservedProjectionId,
    pub(super) observer: Arc<dyn ObservedProjectionSink>,
    delivery: RustObserverDelivery,
}

/// Slot contents: zero or more sink registrations plus a monotonic id allocator.
pub struct ObserverInner {
    rust: Vec<RustObserverRegistration>,
    next_id: u64,
}

impl ObserverInner {
    fn new() -> Self {
        Self {
            rust: Vec::new(),
            next_id: 1,
        }
    }

    fn alloc_id(&mut self) -> ObservedProjectionId {
        let id = ObservedProjectionId(self.next_id);
        self.next_id = self.next_id.wrapping_add(1);
        id
    }
}

/// Shared slot. `Mutex` ensures registration and invocation never tear.
pub type ObservedProjectionSinkSlot = Arc<Mutex<ObserverInner>>;

/// Construct an empty slot for wasm/browser reducer paths.
pub(crate) fn new_event_observer_slot_headless() -> ObservedProjectionSinkSlot {
    Arc::new(Mutex::new(ObserverInner::new()))
}

/// Construct an empty slot.
pub fn new_event_observer_slot() -> ObservedProjectionSinkSlot {
    Arc::new(Mutex::new(ObserverInner::new()))
}

/// In-process observed-projection sink. Implementors carry their own interior
/// mutability because the trait method takes `&self`.
pub trait ObservedProjectionSink: Send + Sync {
    /// Called for replayed events and for future accepted events that match
    /// the sink's declared observed-projection shapes.
    ///
    /// Implementations must be cheap and must not panic: the call site is on
    /// the actor thread between relay frames.
    fn on_kernel_event(&self, event: &KernelEvent);
}

/// Test-only helper for legacy fanout assertions. Production code must use
/// `register_rust_observer_muted` plus `activate_observer_scoped`.
#[cfg(test)]
pub(crate) fn register_rust_observer(
    slot: &ObservedProjectionSinkSlot,
    observer: Arc<dyn ObservedProjectionSink>,
) -> ObservedProjectionId {
    let Ok(mut guard) = slot.lock() else {
        return ObservedProjectionId(0);
    };
    let id = guard.alloc_id();
    guard.rust.push(RustObserverRegistration {
        id,
        observer,
        delivery: RustObserverDelivery::test_all_events(),
    });
    id
}

/// Register an in-process Rust sink in muted state (ADR-0070).
///
/// The sink will not receive events from `notify_observers` until
/// `activate_observer_scoped` is called. It remains addressable by
/// `notify_observer_by_id` during targeted replay.
pub fn register_rust_observer_muted(
    slot: &ObservedProjectionSinkSlot,
    observer: Arc<dyn ObservedProjectionSink>,
) -> ObservedProjectionId {
    let Ok(mut guard) = slot.lock() else {
        return ObservedProjectionId(0);
    };
    let id = guard.alloc_id();
    guard.rust.push(RustObserverRegistration {
        id,
        observer,
        delivery: RustObserverDelivery::Muted,
    });
    id
}

/// Number of registered sink trait-object observers in the slot.
#[must_use]
pub fn rust_observer_count(slot: &ObservedProjectionSinkSlot) -> usize {
    slot.lock().map(|guard| guard.rust.len()).unwrap_or(0)
}

/// Deliver one event to the specific Rust sink identified by `id`, regardless
/// of its muted/scoped state. Used only by the targeted replay path.
pub(crate) fn notify_observer_by_id(
    slot: &ObservedProjectionSinkSlot,
    id: ObservedProjectionId,
    event: &KernelEvent,
) -> bool {
    let observer = {
        let Ok(guard) = slot.lock() else {
            return false;
        };
        guard
            .rust
            .iter()
            .find(|r| r.id == id)
            .map(|r| Arc::clone(&r.observer))
    };
    let Some(observer) = observer else {
        return false;
    };
    let _ = catch_unwind(AssertUnwindSafe(|| observer.on_kernel_event(event)));
    true
}

/// Unregister by id. Idempotent: unknown ids are silent no-ops.
pub fn unregister_observer(slot: &ObservedProjectionSinkSlot, id: ObservedProjectionId) {
    if let Ok(mut guard) = slot.lock() {
        guard.rust.retain(|reg| reg.id != id);
    }
}

/// Fan out one event to every registered scoped sink whose declared shape
/// matches the event. Muted sinks are skipped.
pub(crate) fn notify_observers(slot: &ObservedProjectionSinkSlot, event: &KernelEvent) {
    let rust_snapshot = {
        let Ok(guard) = slot.lock() else {
            return;
        };
        if guard.rust.is_empty() {
            return;
        }
        guard
            .rust
            .iter()
            .filter(|r| r.delivery.matches(event))
            .map(|r| Arc::clone(&r.observer))
            .collect::<Vec<_>>()
    };

    for observer in &rust_snapshot {
        let _ = catch_unwind(AssertUnwindSafe(|| observer.on_kernel_event(event)));
    }
}

#[cfg(test)]
#[path = "event_observer/tests.rs"]
mod tests;
