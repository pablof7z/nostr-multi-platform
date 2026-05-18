//! Relay state transition handlers: connecting / connected / failed / closed.
//!
//! These methods own the side-effects when a transport socket changes state —
//! flipping `RelayStatus.connection`, resetting NIP-42 drivers on disconnect,
//! marking wire-subs as `retrying`/`closed`, and bumping `changed_since_emit`
//! so the actor surfaces the transition in the next snapshot.

use super::super::*;

impl Kernel {
    pub(crate) fn relay_connecting(&mut self, role: RelayRole) {
        let relay = self.relay_mut(role);
        relay.connection = "connecting".to_string();
        self.changed_since_emit = true;
        self.log(format!("connecting {} relay {}", role.key(), role.url()));
    }

    pub(crate) fn relay_connected(&mut self, role: RelayRole) {
        let relay = self.relay_mut(role);
        relay.connection = "connected".to_string();
        relay.connected_at = Some(Instant::now());
        relay.last_error = None;
        relay.auth = "not_required".to_string();
        // T120 (G8 / G11): a fresh socket clears any prior denial — the
        // remote may have changed policy or the user re-paid. The classifier
        // re-stamps `denied` if the new socket also rejects us.
        relay.denied = false;
        relay.last_close_reason = None;
        self.changed_since_emit = true;
        self.log(format!("{} relay connected", role.key()));
        // M5+M2+M8 wiring: on reconnect the NIP-42 driver resets — the relay
        // will re-send a fresh AUTH challenge if it still requires auth.
        if let Some(driver) = self.nip42_drivers.get_mut(&role) {
            driver.reset_on_disconnect();
        }
    }

    pub(crate) fn relay_failed(&mut self, role: RelayRole, error: String) {
        let relay = self.relay_mut(role);
        relay.connection = "backing_off".to_string();
        relay.last_error = Some(truncate(&error, 160));
        relay.reconnect_count = relay.reconnect_count.saturating_add(1);
        self.thread_ids_inflight = false;
        self.thread_replies_inflight = false;
        self.changed_since_emit = true;
        self.log(format!(
            "{} relay error: {}",
            role.key(),
            truncate(&error, 140)
        ));
        for sub in self.wire_subs.values_mut() {
            if sub.role == role && sub.state != "closed" {
                sub.state = "retrying".to_string();
            }
        }
    }

    pub(crate) fn relay_closed(&mut self, role: RelayRole) {
        let relay = self.relay_mut(role);
        relay.connection = "closed".to_string();
        relay.auth = "not_required".to_string();
        // T133: the socket for `role` is gone — every wire-sub on that lane is
        // dead. Evict rather than mark `state="closed"`; the diagnostic value
        // of a row that can never resume is zero, and accumulating closed rows
        // across reconnect churn is exactly the long-session leak T133 fixes.
        // `retrying` (set by `relay_failed`) is preserved — that's a transient
        // state where the sub may resume after backoff.
        self.wire_subs.retain(|_id, sub| sub.role != role);
        self.changed_since_emit = true;
        if let Some(driver) = self.nip42_drivers.get_mut(&role) {
            driver.reset_on_disconnect();
        }
    }
}
