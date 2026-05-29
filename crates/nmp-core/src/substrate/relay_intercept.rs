//! `RelayTextInterceptor` — substrate-generic seam for NIP crates that need
//! to peek at incoming text frames from specific relays.
//!
//! # Why this exists
//!
//! Some NIP-crate runtimes are *response-driven* — they don't fit the
//! command-shaped [`crate::substrate::ProtocolCommand`] seam because their
//! work is triggered by inbound relay frames, not by host commands:
//!
//! * `nmp-nip47` peeks at every text frame from the NWC relay to decode
//!   kind:23195 responses, decrypt the payload, drain `pending_payments`,
//!   and route `pay_invoice` outcomes through
//!   `Kernel::record_action_success` / `..._failure`.
//!
//! Before V-38, the actor's relay-event handler called
//! `commands::handle_nwc_text(wallet, …)` directly — `nmp-core` named the
//! NIP-47 nouns, which is the D0 violation V-38 closes.
//!
//! `RelayTextInterceptor` lifts that hook out of `nmp-core`: the actor
//! reaches into the host-installed slot on every text frame and gives the
//! NIP-crate runtime a chance to intercept. The trait is substrate-generic;
//! the wallet runtime (in `nmp-nip47`) is the first impl.
//!
//! ## Idle-tick hook
//!
//! `on_idle_tick` is called from the actor's idle section on **every**
//! loop iteration (whether or not a relay frame arrived). This is the
//! correct hook for wall-clock-gated sweeps (e.g. pending-payment TTL
//! expiry) that must fire even when the watched relay is silent. The
//! default impl is a no-op so existing interceptors need not change.

use std::sync::{Arc, Mutex};

use crate::kernel::Kernel;
use crate::relay::OutboundMessage;

/// A NIP-crate-owned hook the actor calls for every inbound text frame and
/// on every idle-loop iteration.
///
/// The hook decides for itself whether the frame is "interesting" (e.g.
/// `nmp-nip47` checks `relay_url` against its current NWC connection's
/// relay). Uninteresting frames return an empty `Vec`.
///
/// `Send + Sync` so the slot can be a shared `Arc<dyn …>` cloned to the
/// FFI surface.
pub trait RelayTextInterceptor: Send + Sync + 'static {
    /// Inspect a text frame. Return any outbound frames to enqueue back at
    /// the relay layer (typically empty — the wallet runtime's
    /// kind:23195 decode is read-only against the kernel state).
    ///
    /// `kernel` is mutable so the interceptor can record action terminals,
    /// set the last-error toast, and mark the snapshot dirty without
    /// re-entering through the actor's command channel (which would defer
    /// by at least one tick).
    fn on_relay_text(
        &self,
        kernel: &mut Kernel,
        relay_url: &str,
        text: &str,
    ) -> Vec<OutboundMessage>;

    /// Called from the actor's idle section on every loop iteration,
    /// whether or not a relay frame arrived.
    ///
    /// Use this for wall-clock-gated sweeps (e.g. payment TTL expiry) that
    /// must fire even when the watched relay is silent. The actor drives this
    /// on the same ~250 ms idle cadence as the publish-engine tick. The
    /// default impl is a no-op; override only when a time-gated sweep is
    /// needed.
    ///
    /// D8: no sleep/loop inside — compare `kernel.now_secs()` against stored
    /// insertion timestamps and emit failures for expired entries.
    fn on_idle_tick(&self, _kernel: &mut Kernel) -> Vec<OutboundMessage> {
        Vec::new()
    }
}

/// Shared slot holding active [`RelayTextInterceptor`]s.
///
/// `Arc<Mutex<Vec<Arc<dyn …>>>>` so the slot is host-mutable during app
/// construction without `&mut self` on `NmpApp`, and the inner `Arc`s can be
/// cloned out under the lock and invoked outside it (no long-held mutex around
/// hook bodies).
///
/// Multiple protocol crates can observe the same inbound text frame. Each
/// hook decides whether the frame is relevant to it; uninteresting frames
/// return an empty `Vec`.
pub type RelayTextInterceptorSlot = Arc<Mutex<Vec<Arc<dyn RelayTextInterceptor>>>>;

/// Construct a fresh, empty [`RelayTextInterceptorSlot`].
#[must_use]
pub fn new_relay_text_interceptor_slot() -> RelayTextInterceptorSlot {
    Arc::new(Mutex::new(Vec::new()))
}
