//! Negative no_raw_tap fixture — must produce zero findings.
//!
//! Demonstrates two classes of compliant code:
//!
//! 1. In-process relay forwarding via `ExternalEventSinkPolicy` (retained and
//!    ALLOWED — this is the in-process relay-forwarding policy seam, not the
//!    deleted native push sink).
//! 2. The canonical external mirror pull path: `GlobalLog` cursor +
//!    UniFFI `NmpApp::mirror_pull_page` + `AdvancePullCursor` (ADR-0058).
//!    References to `after_seq`, `AdvancePullCursor`, and
//!    `NmpApp::mirror_pull_page` must NOT be flagged — they are the canonical
//!    replacement for the deleted native sink.

use std::sync::Arc;

/// Canonical in-process relay-forwarding: implement ExternalEventSinkPolicy
/// instead of registering a RawEventObserver or a native push C-ABI sink.
pub struct MyRelayForwardPolicy;

impl MyRelayForwardPolicy {
    pub fn dispatch_frame(&self, json: Arc<str>) {
        // Forward via ExternalEventSinkPolicy + ExternalEventSinkDispatcher,
        // not via the old raw tap or the deleted native push sink.
        let _ = json;
    }
}

/// Canonical external mirror pull path — no banned token here.
/// An external consumer registers a GlobalLog cursor and drains via
/// UniFFI NmpApp::mirror_pull_page; it never registers a push callback.
pub struct MirrorPullState {
    after_seq: u64,
}

impl MirrorPullState {
    /// Called when the host receives an nmp.pull.wake projection.
    /// Calls NmpApp::mirror_pull_page, applies the page, persists after_seq,
    /// then AdvancePullCursor — all through the sanctioned pull seam.
    pub fn on_pull_wake(&mut self, latest_seq: u64) {
        // Pull, apply, advance — no retain_until_ack, no event_sink_watermark.
        if latest_seq > self.after_seq {
            self.after_seq = latest_seq;
        }
    }
}
