//! Kernel request coordination — relay state transitions, startup REQs, view
//! open/close dispatch, and the core `req` / `defer_outbound` / `record_tx`
//! primitives.
//!
//! Per-view request builders are in sibling files:
//! - `profile.rs` — profile/author open/close/claim/release
//! - `thread.rs`  — thread open/close/hydration

mod profile;
mod thread;

use super::*;

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
        for sub in self.wire_subs.values_mut() {
            if sub.role == role {
                sub.state = "closed".to_string();
            }
        }
        self.changed_since_emit = true;
        if let Some(driver) = self.nip42_drivers.get_mut(&role) {
            driver.reset_on_disconnect();
        }
    }

    pub(crate) fn startup_requests(&mut self) -> Vec<OutboundMessage> {
        self.contacts_deadline = Some(Instant::now() + Duration::from_secs(3));
        let seeds = seed_accounts();
        let seed_pubkeys = seeds.iter().map(|seed| seed.pubkey).collect::<Vec<_>>();

        for seed in &seeds {
            self.timeline_authors.insert(seed.pubkey.to_string());
            self.log(format!(
                "seed account: {} {}",
                seed.name,
                short_hex(seed.pubkey)
            ));
        }

        let mut requests = Vec::new();
        requests.push(self.req(
            RelayRole::Content,
            "seed-bootstrap",
            "seed author bootstrap timeline",
            json!({"kinds":[1,6],"authors":seed_pubkeys.clone(),"limit":80}),
        ));
        requests.push(self.req(
            RelayRole::Indexer,
            "profile-target",
            "target kind:0 profile via indexer",
            json!({"kinds":[0],"authors":[TEST_PUBKEY],"limit":1}),
        ));
        requests.push(self.req(
            RelayRole::Indexer,
            "target-relays",
            "target NIP-65 relay list",
            json!({"kinds":[10002],"authors":[TEST_PUBKEY],"limit":1}),
        ));
        requests.push(self.req(
            RelayRole::Indexer,
            "seed-contacts",
            "seed kind:3 contacts via indexer",
            json!({"kinds":[3],"authors":seed_pubkeys.clone(),"limit":10}),
        ));
        requests.push(self.req(
            RelayRole::Indexer,
            "seed-profiles",
            "seed kind:0 profiles via indexer",
            json!({"kinds":[0],"authors":seed_pubkeys.clone(),"limit":20}),
        ));
        requests.push(self.req(
            RelayRole::Indexer,
            "seed-relays",
            "seed NIP-65 relay lists",
            json!({"kinds":[10002],"authors":seed_pubkeys,"limit":10}),
        ));
        self.requested_profiles.insert(TEST_PUBKEY.to_string());
        for seed in seed_accounts() {
            self.requested_profiles.insert(seed.pubkey.to_string());
        }
        requests
    }

    pub(crate) fn active_subscriptions(&self, role: RelayRole) -> Vec<String> {
        self.wire_subs
            .values()
            .filter(|sub| {
                sub.role == role && !matches!(sub.state.as_str(), "closed" | "closed_by_relay")
            })
            .map(|sub| sub.id.clone())
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

    pub(crate) fn close_subscriptions_with_prefixes(
        &mut self,
        prefixes: &[&str],
    ) -> Vec<OutboundMessage> {
        let mut closes = Vec::new();
        for sub in self.wire_subs.values_mut() {
            if prefixes.iter().any(|prefix| sub.id.starts_with(prefix))
                && !matches!(sub.state.as_str(), "closed" | "closed_by_relay")
            {
                sub.state = "closed".to_string();
                sub.close_reason = Some("view closed".to_string());
                closes.push(OutboundMessage {
                    role: sub.role,
                    text: json!(["CLOSE", sub.id]).to_string(),
                });
            }
        }
        closes
    }

    pub(crate) fn req(
        &mut self,
        role: RelayRole,
        sub_id: &str,
        summary: &str,
        filter: Value,
    ) -> OutboundMessage {
        self.log(format!("REQ {sub_id}@{}: {summary}", role.key()));
        let paused = self.relay_auth_paused(role);
        self.wire_subs.insert(
            sub_id.to_string(),
            WireSub {
                id: sub_id.to_string(),
                role,
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
            text: json!(["REQ", sub_id, filter]).to_string(),
        }
    }

    /// True when an inbound `["AUTH", _]` has been received on `role` and the
    /// handshake has not yet completed (`Authenticated`/`NotRequired`/`Failed`
    /// are all pass-through; `Failed` is pass-through because the actor /
    /// operator owns the resolution path per D7).
    pub(crate) fn relay_auth_paused(&self, role: RelayRole) -> bool {
        let state = self
            .nip42_drivers
            .get(&role)
            .map(|d| d.state.clone())
            .unwrap_or(crate::subs::RelayAuthState::NotRequired);
        matches!(
            state,
            crate::subs::RelayAuthState::ChallengeReceived
                | crate::subs::RelayAuthState::Authenticating
        )
    }

    /// Partition an outbound batch: REQ frames targeting an AUTH-paused relay
    /// are removed from the batch and parked in the deferred queue (drained
    /// on `Authenticated` via `pending_view_requests`). Non-REQ frames and
    /// REQs to live relays pass through unchanged. This is the M5+M2+M8
    /// wiring seam replacing the hand-rolled "send + cross-fingers" path for
    /// AUTH-required relays — `AuthGate` semantics modelled inline so the
    /// kernel doesn't need to hold a separate per-relay buffer.
    ///
    /// **D8 invariant:** unlike the generic `defer_outbound` path (which
    /// bumps `changed_since_emit` because connection-drop replay is itself
    /// a diagnostic event worth surfacing), AUTH-pause re-defers do NOT
    /// bump the emit flag. AUTH-state is already pure-diagnostic per the
    /// `update_relay_auth_status` contract; re-defer on every tick (the
    /// `pending_view_requests` drain → still-paused re-defer loop) would
    /// otherwise wake the actor every tick.
    pub(crate) fn partition_auth_paused(
        &mut self,
        outbound: Vec<OutboundMessage>,
    ) -> Vec<OutboundMessage> {
        let mut passthrough = Vec::with_capacity(outbound.len());
        for msg in outbound {
            if msg.text.starts_with("[\"REQ\"") && self.relay_auth_paused(msg.role) {
                self.log(format!("REQ@{} held — relay AUTH-paused", msg.role.key()));
                self.defer_outbound_silent(msg);
            } else {
                passthrough.push(msg);
            }
        }
        passthrough
    }

    /// Diagnostic-quiet variant of `defer_outbound` — same bounded-queue
    /// discipline (64 slots) but does NOT set `changed_since_emit`. Used by
    /// `partition_auth_paused` so the actor doesn't false-wakeup-emit on
    /// every tick that re-defers an AUTH-paused REQ.
    fn defer_outbound_silent(&mut self, message: OutboundMessage) {
        self.deferred_outbound.push_back(message);
        while self.deferred_outbound.len() > 64 {
            self.deferred_outbound.pop_front();
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
