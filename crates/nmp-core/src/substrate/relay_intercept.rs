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

use std::sync::{Arc, Mutex};

use crate::kernel::Kernel;
use crate::relay::OutboundMessage;

/// A NIP-crate-owned hook the actor calls for every inbound text frame.
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
}

/// Shared slot holding the active [`RelayTextInterceptor`].
///
/// `Arc<Mutex<Option<Arc<dyn …>>>>` so the slot is host-mutable
/// (`*slot.lock() = Some(rt)` at app construction) without `&mut self` on
/// `NmpApp`, and the inner `Arc` can be cloned out under the lock and
/// invoked outside it (no long-held mutex around the hook body).
///
/// Only ONE interceptor today — `nmp-nip47`. A future second consumer
/// (e.g. a NIP-44 group relay watcher) gets its own typed slot.
pub type RelayTextInterceptorSlot =
    Arc<Mutex<Option<Arc<dyn RelayTextInterceptor>>>>;

/// Construct a fresh, empty [`RelayTextInterceptorSlot`].
#[must_use]
pub fn new_relay_text_interceptor_slot() -> RelayTextInterceptorSlot {
    Arc::new(Mutex::new(None))
}
