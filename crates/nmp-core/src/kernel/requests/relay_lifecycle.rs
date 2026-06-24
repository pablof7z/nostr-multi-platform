//! Relay state transition handlers: connecting / connected / failed / closed.
//!
//! These methods own the side-effects when a transport socket changes state —
//! flipping `RelayStatus.connection`, resetting NIP-42 drivers on disconnect,
//! marking wire-subs as `retrying`/`closed`, and bumping `changed_since_emit`
//! so the actor surfaces the transition in the next snapshot.

use super::super::{truncate, CanonicalRelayUrl, Instant, Kernel, RelayRole};

impl Kernel {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn relay_connecting(&mut self, role: RelayRole) {
        let relay_url = self
            .bootstrap_urls_for_role(role)
            .first()
            .cloned()
            .unwrap_or_default();
        self.relay_connecting_url(role, &relay_url);
    }

    pub(crate) fn relay_connecting_url(&mut self, role: RelayRole, relay_url: &str) {
        let relay = self.relay_mut(role);
        relay.connection = "connecting".to_string();
        self.mark_transport_connecting(role, relay_url);
        self.changed_since_emit = true;
        self.log(format!("connecting {} relay {}", role.key(), relay_url));
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn relay_connected(&mut self, role: RelayRole) {
        self.mark_lane_connected(role);
        self.log(format!("{} relay connected", role.key()));
        if let Some(driver) = self.auth_drivers.get_mut(&role) {
            driver.reset_on_disconnect();
        }
    }

    pub(crate) fn relay_connected_url(&mut self, role: RelayRole, relay_url: &str) {
        self.mark_lane_connected(role);
        self.mark_transport_connected(role, relay_url);
        self.log(format!("{} relay connected ({relay_url})", role.key()));
        if let Some(driver) = self.auth_drivers.get_mut(&role) {
            driver.reset_on_disconnect();
        }
        // B3 (Workstream B acquisition-one-door) — mailbox-probe re-arm on a
        // GENUINE indexer-lane outage recovery.
        //
        // F-TTL / M2 retry-on-miss: when the indexer lane recovers from a full
        // outage, authors whose kind:10002 we probed once and never got (empty
        // EOSE, or every indexer was down when we probed) deserve a fresh probe.
        // `observe_indexer_lane_health` re-arms the implicit-discovery probed set
        // and bumps the probe epoch — but ONLY on the lane-level `down → up`
        // edge (every indexer socket was down, then one came back).
        //
        // Why lane-level, not the per-socket `was_down` gate this replaces
        // (#1436 + its successor): the per-socket gate re-armed whenever THIS
        // url's row had been `backing_off`/`closed`, so a single flapping
        // indexer re-blasted the whole probe batch even while sibling indexers
        // stayed live the entire time — exactly the per-reconnect churn that
        // starves the wasm UI. The lane epoch fires the re-arm only on a real
        // outage recovery; a reconnect with ≥1 sibling still connected is a
        // no-op. The genuinely-NEW-indexer case (a relay added to the configured
        // set) is still handled separately by `set_configured_relays`'s
        // `IndexerSetChanged` trigger; here we only handle outage recovery.
        if role == RelayRole::Indexer {
            self.observe_indexer_lane_health();
        }
    }

    fn mark_lane_connected(&mut self, role: RelayRole) {
        let relay = self.relay_mut(role);
        relay.connection = "connected".to_string();
        relay.connected_at = Some(Instant::now()); // doctrine-allow: D9 — relay diagnostic elapsed-time marker; not replay policy
        relay.last_error = None;
        // A fresh socket clears any prior typed error category — leaving a
        // stale `error_category` would mislead iOS into branching on an
        // error class that no longer applies (advisor blind-spot fix).
        relay.error_category = None;
        relay.auth = "not_required".to_string();
        // T120 (G8 / G11): a fresh socket clears any prior denial — the
        // remote may have changed policy or the user re-paid. The classifier
        // re-stamps `denied` if the new socket also rejects us.
        relay.denied = false;
        relay.last_close_reason = None;
        self.changed_since_emit = true;
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
        self.mark_transport_failed(role, canonical.as_str(), error.clone());
        let relay = self.relay_mut(role);
        relay.connection = "backing_off".to_string();
        relay.last_error = Some(truncate(&error, 160));
        // A failed transport socket is a transient condition — the reconnect
        // worker will retry. iOS branches on `transient` to show a "retrying"
        // affordance rather than a hard-failure prompt.
        relay.error_category = Some(super::super::closed_reason::ERR_TRANSIENT.to_string());
        relay.reconnect_count = relay.reconnect_count.saturating_add(1);
        // V-112 (ADR-0042): thread_view.ids_inflight / replies_inflight deleted.
        self.changed_since_emit = true;
        self.log(format!(
            "{} relay error ({}): {}",
            role.key(),
            relay_url,
            truncate(&error, 140)
        ));
        for sub in self.wire.subs.values_mut() {
            if sub.relay_url == canonical && sub.state != "closed" {
                sub.state = "retrying".to_string();
            }
        }

        // W5 §8.1 — claim-expansion score hook (relay_failed = Failed, §8.5 +3f).
        // Walk all pending claims and record a Failed outcome for each claim
        // that attempted the failed relay URL. Delegated to `relay_failed_claim_walk`
        // in `claim_expansion.rs` (D4: single writer of the score map).
        self.relay_failed_claim_walk(relay_url);

        // B3 — record the indexer-lane outage edge. `mark_transport_failed`
        // above flipped this URL's row to `backing_off`; if that took the last
        // connected indexer down, the lane is now down and the NEXT recovery
        // bumps the probe epoch (see `relay_connected_url`). No-op otherwise.
        if role == RelayRole::Indexer {
            self.observe_indexer_lane_health();
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
        self.mark_transport_closed(role, canonical.as_str());
        let relay = self.relay_mut(role);
        relay.connection = "closed".to_string();
        relay.auth = "not_required".to_string();
        // T133: the socket for `relay_url` is gone — every wire-sub on that
        // URL is dead. Evict rather than mark `state="closed"`; the
        // diagnostic value of a row that can never resume is zero, and
        // accumulating closed rows across reconnect churn is exactly the
        // long-session leak T133 fixes. Sibling sockets on the same role
        // lane are untouched — their subs are still live.
        self.wire
            .subs
            .retain(|_key, sub| sub.relay_url != canonical);
        self.changed_since_emit = true;
        if let Some(driver) = self.auth_drivers.get_mut(&role) {
            driver.reset_on_disconnect();
        }
        // M2 migration: profile (kind:0) claims are registry interests now, so
        // the planner's reconnect-replay re-emits them automatically — the
        // bespoke `profile_requests` requested→pending re-queue is gone. B3:
        // retry-on-miss for kind:10002 discovery probes is driven by the
        // lane-level outage epoch. `mark_transport_closed` above flipped this
        // URL's row to `closed`; observe the lane so a full indexer outage is
        // recorded and the NEXT recovery re-arms the probe set
        // (`relay_connected_url`). A connect not preceded by a full-lane outage
        // is correctly a no-op (no churn on sibling-still-live reconnects).
        if role == RelayRole::Indexer {
            self.observe_indexer_lane_health();
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
        self.mark_transport_role_closed(role);
        self.wire.subs.retain(|_key, sub| sub.role != role);
        self.changed_since_emit = true;
        if let Some(driver) = self.auth_drivers.get_mut(&role) {
            driver.reset_on_disconnect();
        }
        // B3 — a full role teardown takes the whole indexer lane down. Record
        // the outage edge so a later reconnect (after a Start) re-arms the
        // probe set. `mark_transport_role_closed` above marked every indexer
        // row `closed`, so `any_indexer_connected` is now false.
        if role == RelayRole::Indexer {
            self.observe_indexer_lane_health();
        }
    }
}
