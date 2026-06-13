//! Kernel relay-status and subscription-status projection helpers.
//!
//! Implements `Kernel::relay_status`, `Kernel::relay_statuses`, and
//! `Kernel::subscription_statuses`. These are read-only views over the
//! kernel's relay-connectivity and subscription-lifecycle state — used by
//! `make_update` to populate the diagnostics-screen snapshot projection.

use super::{
    short_hex, BTreeSet, Counters, Instant, Kernel, LogicalInterestStatus, RelayHealth, RelayRole,
    RelayStatus, WireSubscriptionStatus,
};
// `now_hms` is `#[cfg(feature = "native")]` (reads OS wall clock via
// `chrono::Local`). The single use site in `log()` is already gated; the
// import has to match so `--no-default-features` (wasm32) compiles.
#[cfg(feature = "native")]
use super::now_hms;

impl Kernel {
    pub(super) fn relay_status(&self) -> RelayStatus {
        self.relay_status_for(RelayRole::Content)
    }

    pub(super) fn relay_statuses(&self) -> Vec<RelayStatus> {
        let mut statuses: Vec<RelayStatus> = RelayRole::all()
            .into_iter()
            .map(|role| self.relay_status_for(role))
            .collect();
        // Include outbox relay URLs present in wire_subs but not covered by a
        // bootstrap role (T105 — resolved per-author URLs appear here only).
        let known_urls: std::collections::HashSet<&str> =
            statuses.iter().map(|s| s.relay_url.as_str()).collect();
        let outbox_urls: std::collections::BTreeSet<String> = self
            .wire
            .subs
            .values()
            .map(|sub| sub.relay_url.to_string())
            .filter(|url| !known_urls.contains(url.as_str()))
            .collect();
        for url in outbox_urls {
            let active_subs = self
                .wire
                .subs
                .values()
                .filter(|sub| {
                    sub.relay_url == *url.as_str()
                        && !matches!(sub.state.as_str(), "closed" | "closed_by_relay")
                })
                .count();
            let last_event_at_ms = self
                .wire
                .subs
                .values()
                .filter(|sub| sub.relay_url == *url.as_str())
                .filter_map(|sub| self.elapsed_ms(sub.last_event_at))
                .max();
            let events_rx = self
                .wire
                .subs
                .values()
                .filter(|sub| sub.relay_url == *url.as_str())
                .map(|sub| sub.events_rx)
                .sum();
            let info = self.relay_info_for(&url).cloned();
            statuses.push(RelayStatus {
                role: "outbox".to_string(),
                relay_url: url,
                connection: if active_subs > 0 {
                    "connected".to_string()
                } else {
                    "unknown".to_string()
                },
                auth: "—".to_string(),
                negentropy_probe: "unknown".to_string(),
                active_wire_subscriptions: active_subs,
                reconnect_count: 0,
                last_connected_at_ms: None,
                last_event_at_ms,
                last_notice: None,
                last_error: None,
                error_category: None,
                events_rx,
                bytes_rx: 0,
                bytes_tx: 0,
                denied: false,
                last_close_reason: None,
                info,
            });
        }
        statuses
    }

    /// T112 — update the negentropy probe state for a relay lane. Called by
    /// the actor/observer layer when the shell's negentropy capability probe
    /// transitions (`Unknown → Probing → Supported/Unsupported`). Negentropy
    /// itself is a generic relay-side reconciliation capability; the concrete
    /// NIP binding lives in a downstream protocol crate, so this substrate
    /// hook stays NIP-agnostic. The string key must match a probe-state
    /// variant name in `snake_case`: `"unknown"`, `"probing"`, `"supported"`,
    /// or `"unsupported"`.
    ///
    /// `nmp-core` does not name any shell-side probe types (D0); the caller
    /// owns the translation from its probe-state enum to the key string.
    #[allow(dead_code)] // Wired in by actor observer once the shell's CapabilityCache is plumbed
    pub fn set_negentropy_probe_state(&mut self, role: RelayRole, state_key: &str) {
        self.relay_mut(role).negentropy_probe_state = state_key.to_string();
    }

    /// GAP-5 — record a completed negentropy reconciliation session. Called by
    /// the NIP-77 runtime (`nmp-nip77`) on session completion with the raw counts
    /// it observed. Derived fields are computed here (kernel-side per D9):
    /// - `transfer_avoided_bytes = (local_item_count − have_ids) × AVG_EVENT_BYTES`
    /// - `last_reconcile_at_ms` stamped from the injected clock (deterministic
    ///   replay / tests stay consistent because `now_ms()` routes through the
    ///   `Clock` abstraction, never raw `SystemTime::now()`).
    ///
    /// D0: the setter is NIP-agnostic — it takes plain integers so `nmp-core`
    /// does not depend on any `nmp-nip77` type. D4: the kernel is the single
    /// writer of `negentropy_sync_stats`.
    pub fn set_negentropy_sync_stats(
        &mut self,
        rounds: u64,
        have_ids: u64,
        need_ids: u64,
        local_item_count: u64,
    ) {
        use super::types::{AVG_EVENT_BYTES, NegentropySyncStats};
        let transfer_avoided_bytes =
            local_item_count.saturating_sub(have_ids).saturating_mul(AVG_EVENT_BYTES);
        self.negentropy_sync_stats = NegentropySyncStats {
            rounds,
            have_ids,
            need_ids,
            local_item_count,
            transfer_avoided_bytes,
            last_reconcile_at_ms: Some(self.now_ms()),
        };
    }

    pub(super) fn relay_status_for(&self, role: RelayRole) -> RelayStatus {
        let relay = self.relay(role);
        let relay_url = self
            .bootstrap_urls_for_role(role)
            .first()
            .cloned()
            .unwrap_or_default();
        let info = self.relay_info_for(&relay_url).cloned();
        RelayStatus {
            role: role.key().to_string(),
            relay_url,
            connection: relay.connection.clone(),
            auth: relay.auth.clone(),
            negentropy_probe: relay.negentropy_probe_state.clone(),
            active_wire_subscriptions: self
                .wire
                .subs
                .values()
                .filter(|sub| {
                    sub.role == role && !matches!(sub.state.as_str(), "closed" | "closed_by_relay")
                })
                .count(),
            reconnect_count: relay.reconnect_count,
            last_connected_at_ms: self.elapsed_ms(relay.connected_at),
            last_event_at_ms: self.elapsed_ms(relay.last_event_at),
            last_notice: relay.last_notice.clone(),
            last_error: relay.last_error.clone(),
            error_category: relay.error_category.clone(),
            events_rx: relay.counters.events_rx,
            bytes_rx: relay.counters.bytes_rx,
            bytes_tx: relay.counters.bytes_tx,
            denied: relay.denied,
            last_close_reason: relay.last_close_reason.clone(),
            info,
        }
    }

    pub(super) fn logical_interests(&self) -> Vec<LogicalInterestStatus> {
        let mut interests = Vec::new();
        let target_pk = self.active_account.as_deref().unwrap_or("");
        interests.push(LogicalInterestStatus {
            key: format!("Profile({})", short_hex(target_pk)),
            state: if self.profiles.contains_key(target_pk) {
                "complete".to_string()
            } else if self.relay(RelayRole::Indexer).connection == "connected" {
                "tailing".to_string()
            } else {
                "opening".to_string()
            },
            refcount: 1,
            relay_urls: self.bootstrap_urls_for_role(RelayRole::Indexer),
            cache_coverage: self.relay_list_coverage(target_pk),
            warming_until_ms: None,
        });
        interests.push(LogicalInterestStatus {
            key: "Timeline".to_string(),
            state: if !self.timeline.is_empty() {
                "tailing".to_string()
            } else if self.timeline_requested {
                "opening".to_string()
            } else {
                "backfilling".to_string()
            },
            refcount: 1,
            relay_urls: self.bootstrap_discovery_relays(),
            cache_coverage: if self.timeline_requested {
                "partial".to_string()
            } else {
                "unknown".to_string()
            },
            warming_until_ms: None,
        });
        if !self.profile_claims.is_empty() {
            let claimed_authors = self.profile_claims.keys().cloned().collect::<BTreeSet<_>>();
            let claim_count = self
                .profile_claims
                .values()
                .map(BTreeSet::len)
                .sum::<usize>();
            let loaded = claimed_authors
                .iter()
                .filter(|pubkey| self.profiles.contains_key(*pubkey))
                .count();
            let pending = claimed_authors
                .iter()
                .filter(|pubkey| self.profile_requests.pending.contains(*pubkey))
                .count();
            let requested = claimed_authors
                .iter()
                .filter(|pubkey| self.profile_requests.requested.contains(*pubkey))
                .count();
            let active_reqs = self
                .wire
                .subs
                .values()
                .filter(|sub| {
                    sub.id.starts_with("profile-claim-")
                        && !matches!(sub.state.as_str(), "closed" | "closed_by_relay")
                })
                .count();
            let missing = claimed_authors.len().saturating_sub(loaded);
            let state = if missing == 0 {
                "complete"
            } else if active_reqs > 0 {
                "loading"
            } else if pending > 0 {
                "queued"
            } else {
                "tailing"
            };
            interests.push(LogicalInterestStatus {
                key: format!(
                    "UIProfileClaims({claim_count} components / {} pubkeys)",
                    claimed_authors.len()
                ),
                state: state.to_string(),
                refcount: claim_count.min(u32::MAX as usize) as u32,
                relay_urls: self.bootstrap_urls_for_role(RelayRole::Indexer),
                cache_coverage: format!(
                    "{loaded}/{} loaded, {pending} pending, {requested} requested, {active_reqs} active REQs",
                    claimed_authors.len()
                ),
                warming_until_ms: None,
            });
        }
        interests.push(LogicalInterestStatus {
            key: "NetworkDiagnostics".to_string(),
            state: "tailing".to_string(),
            refcount: 1,
            relay_urls: self.bootstrap_discovery_relays(),
            cache_coverage: "local".to_string(),
            warming_until_ms: None,
        });
        // V-68 / V-112 (ADR-0042): AuthorProfile / Thread logical-interest status
        // rows deleted — these interests now live in per-app FlatFeed state.
        // M2 (ADR-0042): the `DiagnosticFirehose(#tag)` logical-interest status
        // row was removed with the `open_firehose_tag` verb; generic
        // `open_interest` feeds surface through the standard registry/wire-sub
        // status rows like every other subscription.
        interests
    }

    pub(super) fn wire_subscriptions(&self) -> Vec<WireSubscriptionStatus> {
        let mut subs = self
            .wire
            .subs
            .values()
            .map(|sub| WireSubscriptionStatus {
                wire_id: sub.id.clone(),
                relay_url: sub.relay_url.to_string(),
                filter_summary: sub.filter_summary.clone(),
                state: sub.state.clone(),
                logical_consumer_count: 1,
                events_rx: sub.events_rx,
                opened_at_ms: self.elapsed_ms(Some(sub.opened_at)).unwrap_or(0),
                last_event_at_ms: self.elapsed_ms(sub.last_event_at),
                eose_at_ms: self.elapsed_ms(sub.eose_at),
                close_reason: sub.close_reason.clone(),
            })
            .collect::<Vec<_>>();
        subs.sort_by(|a, b| a.wire_id.cmp(&b.wire_id));
        subs
    }

    pub(super) fn relay(&self, role: RelayRole) -> &RelayHealth {
        self.relays
            .get(&role)
            .expect("relay health initialized for every role") // doctrine-allow: D6 — RelayRole enum is fixed and the constructor seeds every variant; panicking here means a new role was added without updating the seed (a logic bug, not a runtime error)
    }

    pub(super) fn relay_mut(&mut self, role: RelayRole) -> &mut RelayHealth {
        // Content + Indexer are pre-initialized in Kernel::new(); Wallet is
        // lazily created on first use (not a bootstrap-spawned lane).
        self.relays.entry(role).or_default()
    }

    /// `claim_send_gate` equivalent for the wasm `KernelReducer` path —
    /// returns `true` as soon as **any** relay lane has reported `Connected`.
    ///
    /// Mirrors `actor::relay_mgmt::claim_send_gate` (which reads a
    /// `HashSet<RelayRole>` maintained by the actor loop). On the wasm path
    /// the actor never runs; instead `KernelReducer::handle_relay_connected`
    /// calls `relay_connected_url` → `mark_lane_connected`, which sets
    /// `relay.connection = "connected"`. So the authoritative signal is the
    /// kernel's own per-lane `RelayHealth::connection` field — read-only,
    /// no driver pointers, no out-of-crate imports.
    ///
    /// The wasm claim dispatch (`dispatch_routing::claim_dispatch_from_action`)
    /// uses this to compute `can_send`, matching the native `.any()` semantics
    /// exactly: park-on-false keeps the claim in `profile_requests.pending` /
    /// `pending_event_claims` so `handle_relay_connected` drains it on the
    /// next connect; emit-on-true fans the REQ immediately. Biasing to `false`
    /// rather than guessing "open" (driver `current_socket.is_some()` fires at
    /// dial time, before the kernel learns of `Connected`) avoids the
    /// lost-fetch trap where the outbound REQ is dropped with no re-queue.
    pub(crate) fn any_relay_connected(&self) -> bool {
        self.relays
            .values()
            .any(|health| health.connection == "connected")
    }

    pub(super) fn total_counters(&self) -> Counters {
        let mut total = Counters::default();
        for relay in self.relays.values() {
            total.frames_rx = total.frames_rx.saturating_add(relay.counters.frames_rx);
            total.events_rx = total.events_rx.saturating_add(relay.counters.events_rx);
            total.eose_rx = total.eose_rx.saturating_add(relay.counters.eose_rx);
            total.notices_rx = total.notices_rx.saturating_add(relay.counters.notices_rx);
            total.closed_rx = total.closed_rx.saturating_add(relay.counters.closed_rx);
            total.bytes_rx = total.bytes_rx.saturating_add(relay.counters.bytes_rx);
            total.bytes_tx = total.bytes_tx.saturating_add(relay.counters.bytes_tx);
        }
        total
    }

    pub(super) fn relay_list_coverage(&self, pubkey: &str) -> String {
        match self.mailbox_cache().snapshot(&pubkey.to_string()) {
            Some(parsed) => format!(
                "nip65 r{} w{} b{}",
                parsed.read.len(),
                parsed.write.len(),
                parsed.both.len()
            ),
            None => "nip65 unknown".to_string(),
        }
    }

    // V-112 (ADR-0042): `author_interest_relays` deleted — its only caller was
    // the retired author_view status block. Interest relay routing is the
    // planner's job now (per-NIP-65 routing in the compile pass).

    /// Compute estimated store bytes by scanning all events, profiles, and seed
    /// contacts. This is the O(store) function; use `estimated_store_bytes()`
    /// (the public getter) for the cached version.
    fn compute_estimated_store_bytes(&self) -> usize {
        let event_bytes: usize = self
            .events
            .values()
            .map(|event| {
                event.id.len()
                    + event.author.len()
                    + event.content.len()
                    + event.tags.iter().flatten().map(String::len).sum::<usize>()
                    + 72
            })
            .sum();
        let profile_bytes: usize = self
            .profiles
            .values()
            .map(|profile| {
                profile.event_id.len()
                    + profile.display.len()
                    + profile.picture_url.as_ref().map_or(0, String::len)
                    + profile.nip05.len()
                    + profile.about.len()
                    + 96
            })
            .sum();
        event_bytes + profile_bytes + self.seed_contacts.values().map(Vec::len).sum::<usize>() * 64
    }

    /// Get estimated store bytes, using a cached value if available.
    /// The cache is invalidated (set to None) at every store-mutation site
    /// (events, profiles, seed_contacts inserts). Subsequent calls to this
    /// function recompute the value once and cache it until the next mutation.
    pub(super) fn estimated_store_bytes(&self) -> usize {
        if let Some(v) = self.cached_estimated_store_bytes.get() {
            return v;
        }
        let v = self.compute_estimated_store_bytes();
        self.cached_estimated_store_bytes.set(Some(v));
        v
    }

    pub(super) fn elapsed_ms(&self, instant: Option<Instant>) -> Option<u128> {
        let started = self.timing.started_at?;
        Some(instant?.duration_since(started).as_millis())
    }

    pub(crate) fn log(&mut self, message: impl Into<String>) {
        // `now_hms` is `#[cfg(feature = "native")]` — the wall-clock reader
        // (`chrono::Local`) lives behind the `native` Cargo feature. Under
        // `--no-default-features` the log line still records the message;
        // only the leading `HH:MM:SS` timestamp drops (the kernel still
        // owns logical ordering via the bounded ring buffer below).
        #[cfg(feature = "native")]
        let stamp = now_hms();
        #[cfg(not(feature = "native"))]
        let stamp = "";
        let line = format!("{stamp} {}", message.into());
        // D6: library code performs no I/O side effects. The line is kept in
        // the bounded ring buffer below; it surfaces via the kernel snapshot
        // (D7: the kernel reports, the platform decides what to show).
        self.logs.push_back(line);
        while self.logs.len() > 80 {
            self.logs.pop_front();
        }
    }
}

// T112 — negentropy probe-state projection tests. (The probe itself is the
// generic negentropy-capability handshake; NIP-77 is the per-relay binding
// that lives in a downstream protocol crate, not the substrate.)
#[cfg(test)]
mod negentropy_probe_status_tests {
    use super::*;
    use crate::relay::DEFAULT_VISIBLE_LIMIT;

    #[test]
    fn t112_negentropy_probe_state_projected_into_relay_status() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

        // Default: both bootstrap roles report "unknown".
        let statuses = kernel.relay_statuses();
        for s in &statuses {
            if s.role == "content" || s.role == "indexer" {
                assert_eq!(
                    s.negentropy_probe, "unknown",
                    "default probe state must be 'unknown' for role {}",
                    s.role
                );
            }
        }

        // After the actor/observer calls set_negentropy_probe_state, the projection
        // reflects the new state on the matching lane.
        kernel.set_negentropy_probe_state(RelayRole::Content, "probing");
        let statuses = kernel.relay_statuses();
        let content_row = statuses
            .iter()
            .find(|s| s.role == "content")
            .expect("content relay row must be present");
        assert_eq!(
            content_row.negentropy_probe, "probing",
            "relay_statuses() must reflect the updated probe state on the content lane"
        );

        // Indexer lane is unaffected.
        let indexer_row = statuses
            .iter()
            .find(|s| s.role == "indexer")
            .expect("indexer relay row must be present");
        assert_eq!(
            indexer_row.negentropy_probe, "unknown",
            "indexer lane must remain 'unknown' after updating only the content lane"
        );

        // Terminal states round-trip correctly.
        kernel.set_negentropy_probe_state(RelayRole::Content, "supported");
        let statuses = kernel.relay_statuses();
        let content_row = statuses.iter().find(|s| s.role == "content").unwrap();
        assert_eq!(content_row.negentropy_probe, "supported");

        kernel.set_negentropy_probe_state(RelayRole::Content, "unsupported");
        let statuses = kernel.relay_statuses();
        let content_row = statuses.iter().find(|s| s.role == "content").unwrap();
        assert_eq!(content_row.negentropy_probe, "unsupported");
    }
}
