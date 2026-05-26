//! T146 — `Kernel` integration for the shared `KernelEventObserverSlot`.
//!
//! The slot itself + its registration helpers live in
//! `actor/commands/event_observer.rs`. This file is the kernel-side
//! integration layer:
//!
//! - `set_event_observers_handle` — actor calls this once after building a
//!   kernel, binding the shared `Arc<Mutex<…>>` so the kernel can fan out
//!   events without crossing FFI on each one.
//! - `take_event_observers_handle_for_reset` — preserves the slot across
//!   `ActorCommand::Reset` so existing per-app crate registrations stay
//!   alive (same survival pattern as `dispatch_drops_handle`).
//! - `notify_event_observers` — fan-out entry called after every
//!   observer-visible `EventStore::insert` returning `Inserted | Replaced`.
//!
//! Lives as a sibling of `kernel/mod.rs` to keep `mod.rs` under the
//! AGENTS.md soft cap (300 LOC) — the methods are otherwise inline `impl
//! Kernel` items; splitting them out costs nothing at the call site (D0 —
//! per-app crates compose; the kernel emits, never names a NIP type).
//! ADR-0009.

use super::Kernel;
use crate::actor::KernelEventObserverSlot;
use crate::substrate::KernelEvent;

impl Kernel {
    /// Install the actor's shared kernel event observer slot. The
    /// `Arc<Mutex<…>>` is shared with the FFI surface
    /// (`ffi/event_observer.rs`) and any per-app crate that has called
    /// `NmpApp::event_observers_slot()`; the same registrations are
    /// therefore visible to both the actor thread and external Rust
    /// callers. Idempotent — re-binding replaces the prior handle (so
    /// existing registrations on the old slot become unreachable from the
    /// kernel; callers that hold the prior `Arc` keep their own view). The
    /// actor calls this once immediately after constructing a kernel.
    pub(crate) fn set_event_observers_handle(&mut self, handle: KernelEventObserverSlot) {
        self.event_observers = Some(handle);
    }

    /// Extract the event observer handle before a `Reset` replaces the
    /// kernel. The slot's `Arc<Mutex<…>>` is shared with the FFI surface
    /// and per-app crates, so it MUST survive Reset (otherwise every
    /// registration would silently stop firing).
    pub(crate) fn take_event_observers_handle_for_reset(
        &mut self,
    ) -> Option<KernelEventObserverSlot> {
        self.event_observers.take()
    }

    /// Fan one accepted event out to every registered observer. Called
    /// from the ingest paths (and the test-support fixture) after
    /// `EventStore::insert` returns `Inserted | Replaced`. Best-effort:
    /// missing slot, poisoned mutex, or serialization failure on the
    /// C-ABI side are all silent no-ops (D6). The no-observers fast path
    /// is branch-free — no allocation, no lock taken past the slot's
    /// `Option`.
    ///
    /// `KernelEvent` is the FFI-stable shape from `substrate::view`; the
    /// caller composes it from the kernel's `StoredEvent` (same fields,
    /// just cloned into the FFI struct).
    pub(in crate::kernel) fn notify_event_observers(&self, event: &KernelEvent) {
        if let Some(slot) = &self.event_observers {
            crate::actor::notify_observers(slot, event);
        }
    }
}
