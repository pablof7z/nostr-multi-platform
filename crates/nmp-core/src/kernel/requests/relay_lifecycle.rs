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
        self.log(format!("connecting {} relay {}", role.key(), self.bootstrap_urls_for_role(role).first().cloned().unwrap_or_default()));
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

    /// A transport socket for `role` failed (transient — backoff + retry).
    ///
    /// `relay_url` identifies the *specific* socket that failed. Under T105
    /// URL-keyed routing many sockets share one `RelayRole` lane, so the
    /// `retrying` mark must be scoped to wire-subs opened on **this URL** —
    /// a role-wide mark would wrongly flag healthy sibling sockets' subs as
    /// retrying. The per-lane `RelayStatus` fields stay role-scoped (they are
    /// a lane-level diagnostic surface, not per-URL until M11).
    pub(crate) fn relay_failed(&mut self, role: RelayRole, relay_url: &str, error: String) {
        let canonical = CanonicalRelayUrl::parse_or_raw(relay_url);
        let relay = self.relay_mut(role);
        relay.connection = "backing_off".to_string();
        relay.last_error = Some(truncate(&error, 160));
        relay.reconnect_count = relay.reconnect_count.saturating_add(1);
        self.thread_view.ids_inflight = false;
        self.thread_view.replies_inflight = false;
        self.changed_since_emit = true;
        self.log(format!(
            "{} relay error ({}): {}",
            role.key(),
            relay_url,
            truncate(&error, 140)
        ));
        for sub in self.wire_subs.values_mut() {
            if sub.relay_url == canonical && sub.state != "closed" {
                sub.state = "retrying".to_string();
            }
        }
    }

    /// A transport socket for `role` was fully torn down (no retry).
    ///
    /// `relay_url` identifies the specific socket. T133 eviction must be
    /// scoped to wire-subs opened on **this URL**, not the whole role lane:
    /// post-T105 several sockets share a lane, so a role-wide `retain` would
    /// silently evict live subscriptions belonging to healthy sibling
    /// sockets — a correctness bug, not just a leak. For the global pool
    /// drain (Stop / Reset / Shutdown) use [`Self::relay_closed_all`].
    pub(crate) fn relay_closed(&mut self, role: RelayRole, relay_url: &str) {
        let canonical = CanonicalRelayUrl::parse_or_raw(relay_url);
        let relay = self.relay_mut(role);
        relay.connection = "closed".to_string();
        relay.auth = "not_required".to_string();
        // T133: the socket for `relay_url` is gone — every wire-sub on that
        // URL is dead. Evict rather than mark `state="closed"`; the
        // diagnostic value of a row that can never resume is zero, and
        // accumulating closed rows across reconnect churn is exactly the
        // long-session leak T133 fixes. Sibling sockets on the same role
        // lane are untouched — their subs are still live.
        self.wire_subs.retain(|_key, sub| sub.relay_url != canonical);
        self.changed_since_emit = true;
        if let Some(driver) = self.nip42_drivers.get_mut(&role) {
            driver.reset_on_disconnect();
        }
    }

    /// Global socket teardown for `role` (Stop / Reset / Shutdown): unlike the
    /// per-URL [`Self::relay_closed`], this evicts EVERY wire-sub on the role
    /// lane regardless of URL. Correct only when the whole pool is being
    /// drained — `close_relays` shuts down every socket of every role, so
    /// per-URL scoping would buy nothing and would force the caller to
    /// enumerate sockets it is about to discard anyway.
    pub(crate) fn relay_closed_all(&mut self, role: RelayRole) {
        let relay = self.relay_mut(role);
        relay.connection = "closed".to_string();
        relay.auth = "not_required".to_string();
        self.wire_subs.retain(|_key, sub| sub.role != role);
        self.changed_since_emit = true;
        if let Some(driver) = self.nip42_drivers.get_mut(&role) {
            driver.reset_on_disconnect();
        }
    }
}
