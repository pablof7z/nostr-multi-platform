//! Kernel request coordination — `req` / `req_for_relay` / `defer_outbound` /
//! `record_tx` primitives plus the per-tick view-request dispatcher.
//!
//! Logical groupings are split across sibling files:
//! - `relay_lifecycle.rs` — connecting/connected/failed/closed transitions
//! - `startup.rs`         — cold-start REQ emission (seed bootstrap + self profile)
//! - `auth_gate.rs`       — NIP-42 AUTH paused/failed predicates + outbound partition
//! - `profile.rs`         — profile/author open/close/claim/release
//! - `thread.rs`          — thread open/close/hydration

mod auth_gate;
mod profile;
mod relay_lifecycle;
mod startup;
mod thread;

use super::*;

impl Kernel {
    #[allow(dead_code)] // Per-lane snapshot retained for diagnostic surface (M11).
    pub(crate) fn active_subscriptions(&self, role: RelayRole) -> Vec<String> {
        self.wire_subs
            .values()
            .filter(|sub| {
                sub.role == role && !matches!(sub.state.as_str(), "closed" | "closed_by_relay")
            })
            .map(|sub| sub.id.clone())
            .collect()
    }

    /// Snapshot every active wire-sub as `(sub_id, relay_url)`. T105: the
    /// actor's lane-by-lane close path needs the URL each sub was opened on
    /// so the CLOSE can be routed to the right socket in the URL-keyed
    /// transport pool (the role alone is not enough — many sockets share
    /// one lane).
    pub(crate) fn snapshot_active_wire_subs(&self) -> Vec<(String, String)> {
        self.wire_subs
            .values()
            .filter(|sub| !matches!(sub.state.as_str(), "closed" | "closed_by_relay"))
            .map(|sub| (sub.id.clone(), sub.relay_url.clone()))
            .collect()
    }

    pub(crate) fn pending_view_requests(&mut self) -> Vec<OutboundMessage> {
        let mut requests = Vec::new();
        while let Some(message) = self.deferred_outbound.pop_front() {
            requests.push(message);
        }
        // Check time-gated timeline open (contacts_deadline may have elapsed).
        requests.extend(self.maybe_open_timeline());
        if self.author_request_pending {
            requests.extend(self.author_requests());
        }
        if self.thread_request_pending {
            requests.extend(self.prepare_thread_requests());
        }
        if self.diagnostic_firehose.is_some()
            && !self
                .wire_subs
                .keys()
                .any(|sub_id| sub_id.starts_with("diag-firehose-"))
        {
            requests.extend(self.firehose_requests());
        }
        requests.extend(self.pending_profile_claim_requests());
        requests.extend(self.maybe_open_thread_hydration());
        // T82: turn referenced-but-missing ids collected during ingest into
        // oneshot fetches (idempotent — no-op when the set is empty).
        requests.extend(self.drain_unknown_oneshots());
        requests
    }

    /// Close every wire-sub whose id matches one of `prefixes`, returning the
    /// CLOSE frames to dispatch.
    ///
    /// T133: rows are evicted from `wire_subs` (`HashMap::remove`) once the
    /// CLOSE outbound is constructed. Pre-T133 the row stayed with
    /// `state="closed"` for diagnostic surfacing — under long-running sessions
    /// this let the row table grow unbounded (every profile-claim, thread, or
    /// author view adds rows; close cycles never reclaimed them). Eviction is
    /// O(1) per row (`HashMap::remove`); no per-event alloc on the hot path
    /// (D8 invariant — the close path is cold relative to EVENT ingest).
    pub(crate) fn close_subscriptions_with_prefixes(
        &mut self,
        prefixes: &[&str],
    ) -> Vec<OutboundMessage> {
        // Two-pass: can't `remove` while holding a `&mut` iterator on the map.
        let mut closes = Vec::new();
        let mut to_evict: Vec<String> = Vec::new();
        for sub in self.wire_subs.values() {
            if prefixes.iter().any(|prefix| sub.id.starts_with(prefix))
                && !matches!(sub.state.as_str(), "closed" | "closed_by_relay")
            {
                closes.push(OutboundMessage {
                    role: sub.role,
                    relay_url: sub.relay_url.clone(),
                    text: json!(["CLOSE", sub.id]).to_string(),
                });
                to_evict.push(sub.id.clone());
            }
        }
        for sub_id in to_evict {
            self.wire_subs.remove(&sub_id);
        }
        if !closes.is_empty() {
            self.changed_since_emit = true;
        }
        closes
    }

    /// Build a single REQ frame on `role`'s cold-start bootstrap socket.
    ///
    /// T105 transition shim: kept for diagnostic / one-off REQs (NIP-65
    /// discovery, indexer-only fetches) that legitimately leave on the
    /// bootstrap lane. Per-author/recipient view emitters use
    /// [`Self::req_for_relay`] to route to the planner-resolved URL instead.
    pub(crate) fn req(
        &mut self,
        role: RelayRole,
        sub_id: &str,
        summary: &str,
        filter: Value,
    ) -> OutboundMessage {
        self.req_for_relay(role, role.bootstrap_url().to_string(), sub_id, summary, filter)
    }

    /// Build a single REQ frame addressed to `relay_url` on transport lane `role`.
    ///
    /// T105: the resolved per-author write relay (content/profile/thread) or
    /// recipient read relay (inbox notifications) is threaded straight onto
    /// the wire — the `RelayRole` only labels the diagnostic lane the frame
    /// belongs to. The recorded `WireSub` remembers `relay_url` so the EOSE
    /// CLOSE re-routes to the same socket the REQ went out on.
    pub(crate) fn req_for_relay(
        &mut self,
        role: RelayRole,
        relay_url: String,
        sub_id: &str,
        summary: &str,
        filter: Value,
    ) -> OutboundMessage {
        self.log(format!(
            "REQ {sub_id}@{} ({}): {summary}",
            role.key(),
            relay_url
        ));
        let paused = self.relay_auth_paused(role);
        self.wire_subs.insert(
            sub_id.to_string(),
            WireSub {
                id: sub_id.to_string(),
                role,
                relay_url: relay_url.clone(),
                filter_summary: summary.to_string(),
                state: if paused { "auth_paused" } else { "opening" }.to_string(),
                opened_at: Instant::now(),
                last_event_at: None,
                eose_at: None,
                close_reason: None,
            },
        );
        self.changed_since_emit = true;
        OutboundMessage {
            role,
            relay_url,
            text: json!(["REQ", sub_id, filter]).to_string(),
        }
    }

    pub(crate) fn defer_outbound(&mut self, message: OutboundMessage) {
        self.log(format!(
            "defer {} outbound until relay reconnects",
            message.role.key()
        ));
        self.deferred_outbound.push_back(message);
        while self.deferred_outbound.len() > 64 {
            self.deferred_outbound.pop_front();
        }
        self.changed_since_emit = true;
    }

    pub(crate) fn record_tx(&mut self, role: RelayRole, bytes: usize) {
        let relay = self.relay_mut(role);
        relay.counters.bytes_tx = relay.counters.bytes_tx.saturating_add(bytes as u64);
    }
}
