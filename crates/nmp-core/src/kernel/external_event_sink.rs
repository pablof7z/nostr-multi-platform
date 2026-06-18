//! `Kernel` integration for the `ExternalEventSinkDispatcher`.
//!
//! This is the kernel-side integration layer for the dispatcher introduced in
//! Step 1.  The dispatcher replaces the old `raw_event_observer.rs` slot as
//! the single fan-out point for verbatim inbound signed events.
//!
//! - `set_external_event_sink_dispatcher` — actor calls this once after
//!   building a kernel, binding the dispatcher so the kernel can fan out
//!   signed events off the actor thread.
//!
//! D0 — the kernel never names a NIP / protocol; this is a generic
//! verbatim-signed-event seam. ADR-0009.

use super::Kernel;
use crate::substrate::ExternalEventSinkDispatcher;

impl Kernel {
    /// Install the dispatcher.  Called once from `run_actor_with_observers`
    /// after construction and re-called from the `Reset` arm so the new
    /// kernel shares the same dispatcher (which owns a background thread).
    pub(crate) fn set_external_event_sink_dispatcher(
        &mut self,
        dispatcher: ExternalEventSinkDispatcher,
    ) {
        self.external_event_sink_dispatcher = Some(dispatcher);
    }

    /// Borrow the dispatcher for use in `persistence.rs`.
    pub(in crate::kernel) fn external_event_sink_dispatcher(
        &self,
    ) -> Option<&ExternalEventSinkDispatcher> {
        self.external_event_sink_dispatcher.as_ref()
    }
}
