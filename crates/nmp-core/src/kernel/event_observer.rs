//! `Kernel` integration for the shared `ObservedProjectionSinkSlot`.
//!
//! The slot itself + its registration helpers live in
//! `actor/commands/event_observer.rs`. This file is the kernel-side
//! integration layer:
//!
//! - `set_event_observers_handle` — actor calls this once after building a
//!   kernel, binding the shared `Arc<Mutex<…>>` so the kernel can deliver
//!   events to scoped observed-projection sinks without crossing FFI.
//! - `take_event_observers_handle_for_reset` — preserves the slot across
//!   `ActorCommand::Lifecycle(LifecycleCommand::Reset)` so existing per-app crate registrations stay
//!   alive (same survival pattern as `queue_depth_handle`).
//! - `notify_event_observers` — scoped delivery entry called after every
//!   observer-visible `EventStore::insert` returning `Inserted | Replaced`.
//!
//! Lives as a sibling of `kernel/mod.rs` to keep `mod.rs` under the
//! AGENTS.md soft cap (300 LOC) — the methods are otherwise inline `impl
//! Kernel` items; splitting them out costs nothing at the call site (D0 —
//! per-app crates compose; the kernel emits, never names a NIP type).
//! ADR-0072.

use super::Kernel;
use crate::actor::ObservedProjectionId;
use crate::actor::ObservedProjectionSinkSlot;
use crate::substrate::KernelEvent;

impl Kernel {
    /// Install the actor's shared observed-projection sink slot. The
    /// `Arc<Mutex<…>>` is shared with the FFI surface and per-app Rust
    /// composition, so the same scoped sink registrations are visible to the
    /// actor thread. Idempotent — re-binding replaces the prior handle (so
    /// existing registrations on the old slot become unreachable from the
    /// kernel; callers that hold the prior `Arc` keep their own view). The
    /// actor calls this once immediately after constructing a kernel.
    pub(crate) fn set_event_observers_handle(&mut self, handle: ObservedProjectionSinkSlot) {
        self.event_observers = Some(handle);
    }

    /// Extract the observed-projection sink handle before a `Reset` replaces
    /// the kernel. The slot's `Arc<Mutex<…>>` is shared with the FFI surface
    /// and per-app crates, so it MUST survive Reset (otherwise every
    /// registration would silently stop firing).
    pub(crate) fn take_event_observers_handle_for_reset(
        &mut self,
    ) -> Option<ObservedProjectionSinkSlot> {
        self.event_observers.take()
    }

    /// Deliver one accepted event to every matching scoped sink. Called
    /// from the ingest paths (and the test-support fixture) after
    /// `EventStore::insert` returns `Inserted | Replaced`. Best-effort:
    /// missing slot, poisoned mutex, or serialization failure on the
    /// callback are all silent no-ops (D6). The no-sinks fast path
    /// returns before provenance lookup or allocation.
    ///
    /// `KernelEvent` is the FFI-stable shape from `substrate::view`; the
    /// caller composes it from the kernel's `StoredEvent` (same fields,
    /// just cloned into the FFI struct).
    pub(in crate::kernel) fn notify_event_observers(&self, event: &KernelEvent) {
        let Some(slot) = &self.event_observers else {
            return;
        };
        let mut event = event.clone();
        if event.relay_provenance.is_empty() {
            event.relay_provenance =
                super::provenance::relay_urls_for_event(&*self.store, &event.id);
        }
        crate::actor::notify_observers(slot, &event);
    }

    /// Deliver one event to a single named observer (ADR-0070 targeted replay).
    ///
    /// Enriches `relay_provenance` from the store when empty (same as
    /// `notify_event_observers`), then calls `notify_observer_by_id` — which
    /// delivers regardless of the observer's `active` flag, so this works for
    /// muted registrations installed before catch-up replay.
    ///
    /// Returns `true` iff the registration was found. D6 — missing slot,
    /// poisoned mutex, and observer panics are all silent no-ops.
    pub(in crate::kernel) fn notify_event_observer_by_id(
        &self,
        id: ObservedProjectionId,
        event: &KernelEvent,
    ) -> bool {
        let Some(slot) = &self.event_observers else {
            return false;
        };
        let mut event = event.clone();
        if event.relay_provenance.is_empty() {
            event.relay_provenance =
                super::provenance::relay_urls_for_event(&*self.store, &event.id);
        }
        crate::actor::notify_observer_by_id(slot, id, &event)
    }
}
