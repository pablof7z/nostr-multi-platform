//! Publish-control UniFFI methods.
//!
//! `retry_publish` and `cancel_action` are publish-lifecycle control-plane
//! operations — neither produces events nor goes through `dispatch_action`.
//!
//! D8: both methods are non-blocking channel sends to the actor.

use crate::NmpApp;

#[uniffi::export]
impl NmpApp {
    /// Retry a failed publish, addressed by its handle.
    ///
    /// This is the intentional control-plane door for the publish lifecycle —
    /// `dispatch_action` deliberately does NOT carry retry; the generic action
    /// seam is for *content* actions while publish retry stays on this
    /// dedicated symbol.
    ///
    /// An empty handle is a silent no-op (D6). D8: non-blocking channel send.
    pub fn retry_publish(&self, handle: String) {
        if handle.is_empty() {
            return;
        }
        self.inner.retry_publish(handle);
    }

    /// Cancel an in-flight operation, addressed by its dispatch
    /// `correlation_id` (S7, #1754).
    ///
    /// The kernel reverse-resolves the publish handle from the durable
    /// handle↔correlation index and records a user-initiated `Cancelled`
    /// terminal under the ORIGINAL `correlation_id` (PD-036). A raw publish
    /// handle is also accepted (the index self-maps it).
    ///
    /// An empty `correlation_id` is a silent no-op (D6). D8: non-blocking
    /// channel send.
    pub fn cancel_action(&self, correlation_id: String) {
        if correlation_id.is_empty() {
            return;
        }
        self.inner.cancel_publish(correlation_id);
    }
}
