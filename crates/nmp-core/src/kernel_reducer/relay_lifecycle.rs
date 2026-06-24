//! Relay-lifecycle and maintenance methods for `KernelReducer`.

use crate::kernel::RelayFrame;
use crate::relay::OutboundMessage;
use crate::time::Instant;
use nmp_network::role::RelayRole;

impl super::KernelReducer {
    /// One inbound relay frame on `(role, relay_url)`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn handle_relay_frame(
        &mut self,
        role: RelayRole,
        relay_url: &str,
        frame: RelayFrame,
    ) -> Vec<OutboundMessage> {
        self.handle_relay_frame_at(
            role,
            relay_url,
            frame,
            crate::kernel::test_support::test_support_now(),
        )
    }

    pub fn handle_relay_frame_at(
        &mut self,
        role: RelayRole,
        relay_url: &str,
        frame: RelayFrame,
        now: Instant,
    ) -> Vec<OutboundMessage> {
        let mut outbound = self.kernel.handle_message_at(role, relay_url, frame, now);
        outbound.extend(self.kernel.pending_view_requests_at(now));
        self.kernel.partition_auth_paused(outbound)
    }

    /// A relay socket entered the `connected` state.
    #[cfg(any(test, feature = "test-support"))]
    pub fn handle_relay_connected(
        &mut self,
        role: RelayRole,
        relay_url: &str,
        is_reconnect: bool,
    ) -> Vec<OutboundMessage> {
        self.handle_relay_connected_at(
            role,
            relay_url,
            is_reconnect,
            crate::kernel::test_support::test_support_now(),
        )
    }

    pub fn handle_relay_connected_at(
        &mut self,
        role: RelayRole,
        relay_url: &str,
        is_reconnect: bool,
        now: Instant,
    ) -> Vec<OutboundMessage> {
        self.kernel.relay_connected_url(role, relay_url);
        let mut outbound = Vec::new();
        if is_reconnect {
            outbound.extend(self.kernel.replay_on_reconnect(role, relay_url));
        }
        outbound.extend(self.kernel.mark_publish_relay_available(relay_url));
        outbound.extend(self.kernel.startup_requests(now));
        outbound.extend(self.kernel.pending_view_requests_at(now));
        outbound.extend(self.kernel.drain_lifecycle_outbound());
        self.kernel.partition_auth_paused(outbound)
    }

    /// A relay socket failed transiently.
    pub fn handle_relay_failed(&mut self, role: RelayRole, relay_url: &str, error: String) {
        self.kernel.relay_failed(role, relay_url, error);
        self.kernel.mark_publish_relay_unavailable(relay_url);
    }

    /// A relay socket was torn down.
    pub fn handle_relay_closed(&mut self, role: RelayRole, relay_url: &str) {
        self.kernel.relay_closed(role, relay_url);
        self.kernel.mark_publish_relay_unavailable(relay_url);
    }

    /// Pump all four maintenance drains in native parity order.
    #[cfg(any(test, feature = "test-support"))]
    pub fn tick(&mut self) -> Vec<OutboundMessage> {
        self.tick_at(crate::kernel::test_support::test_support_now())
    }

    pub fn tick_at(&mut self, now: Instant) -> Vec<OutboundMessage> {
        let mut outbound = self.kernel.pending_view_requests_at(now);
        outbound.extend(self.kernel.drain_lifecycle_outbound());
        outbound.extend(self.kernel.poll_claim_expansion(now));
        outbound.extend(self.kernel.tick_publish_engine_for_now());
        self.kernel.partition_auth_paused(outbound)
    }

    /// Return the next kernel-owned runtime deadline, as a delay from now.
    #[must_use]
    pub fn next_runtime_deadline_delay_ms(&self) -> Option<u32> {
        self.kernel.next_publish_engine_deadline_delay_ms()
    }
}
