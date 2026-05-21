//! `Kernel` integration for the shared `RawEventObserverSlot`.
//!
//! The slot itself + its registration / filter helpers live in
//! `actor/commands/raw_event_observer.rs`. This file is the kernel-side
//! integration layer (sibling of `kernel/event_observer.rs`, same shape):
//!
//! - `set_raw_event_observers_handle` — actor calls this once after
//!   building a kernel, binding the shared `Arc<Mutex<…>>` so the kernel
//!   can fan out verbatim signed events without crossing FFI on each one.
//! - `take_raw_event_observers_handle_for_reset` — preserves the slot
//!   across `ActorCommand::Reset` so external registrations stay alive
//!   (same survival pattern as the kernel-event observer slot).
//! - `raw_event_observers_idle_for_kind` — branch-free fast-path probe the
//!   single ingest call-site checks BEFORE re-deriving / serializing the
//!   verbatim event. When no registration filters on `kind` (or the slot
//!   is unbound) the tap is a zero-cost no-op.
//! - `notify_raw_event_observers` — fan-out entry called from the single
//!   all-kinds ingest point after the event has passed the kernel's existing
//!   Schnorr + id-hash gate and the store provenance path.
//!
//! D0 — the kernel never names a NIP / protocol; this is a generic
//! verbatim-signed-event seam. ADR-0009.

use super::Kernel;
use crate::actor::RawEventObserverSlot;
use crate::store::RawEvent;

impl Kernel {
    /// Install the actor's shared raw signed-event tap slot. The
    /// `Arc<Mutex<…>>` is shared with the FFI surface
    /// (`ffi/raw_event_tap.rs`) and any per-app crate that registered a
    /// raw observer; the same registrations are therefore visible to both
    /// the actor thread and external Rust callers. The actor calls this
    /// once immediately after constructing a kernel.
    pub(crate) fn set_raw_event_observers_handle(&mut self, handle: RawEventObserverSlot) {
        self.raw_event_observers = Some(handle);
    }

    /// Extract the raw tap handle before a `Reset` replaces the kernel.
    /// The slot's `Arc<Mutex<…>>` is shared with the FFI surface and
    /// per-app crates, so it MUST survive Reset (otherwise every raw
    /// registration would silently stop firing).
    pub(crate) fn take_raw_event_observers_handle_for_reset(
        &mut self,
    ) -> Option<RawEventObserverSlot> {
        self.raw_event_observers.take()
    }

    /// `true` when no raw-tap registration would accept `kind` (or the
    /// slot is unbound / poisoned). The single ingest call-site checks
    /// this first so the verbatim re-serialization + duplicate Schnorr
    /// verify are skipped entirely on the hot path when nobody is tapping
    /// that kind. Branch-free no-observers fast path (D8): one `Option`
    /// check, then at most one mutex acquire + `is_empty()` scan.
    pub(in crate::kernel) fn raw_event_observers_idle_for_kind(&self, kind: u32) -> bool {
        match &self.raw_event_observers {
            Some(slot) => crate::actor::raw_observers_idle_for_kind(slot, kind),
            None => true,
        }
    }

    /// Fan one verbatim signed event out to every raw-tap registration
    /// whose kind filter matches `raw.kind`. Called from the single
    /// all-kinds ingest point (`kernel/ingest/mod.rs::handle_event`) only
    /// for events that passed the kernel's existing Schnorr + id-hash gate
    /// and store provenance path. Best-effort: unbound slot, poisoned mutex,
    /// or serialization failure on the C-ABI side are all silent no-ops (D6).
    /// The payload
    /// is the byte-faithful flat NIP-01 object
    /// `{id, pubkey, created_at, kind, tags, content, sig}` — the `sig` is
    /// preserved verbatim (this is the whole point of the seam).
    pub(in crate::kernel) fn notify_raw_event_observers(&self, raw: &RawEvent, relay_url: &str) {
        if let Some(slot) = &self.raw_event_observers {
            crate::actor::notify_raw_observers(slot, raw, Some(relay_url));
        }
    }
}
