//! URL-level relay transport diagnostics.
//!
//! `RelayHealth` is role-lane state (`content`, `indexer`, `wallet`). The
//! native/browser transport pools are URL-keyed, so diagnostics need a second
//! projection that keeps one row per actual socket URL while preserving the
//! legacy `RelayStatus` shape consumed by shells.

use std::collections::{BTreeMap, HashSet, VecDeque};

use super::{
    CanonicalRelayUrl, Counters, Instant, Kernel, NoticeEntry, RelayRole, RelayStatus, WireSub,
    MAX_NOTICE_LOG,
};
use crate::substrate::RelayInfoDoc;

#[derive(Clone, Debug, Default)]
pub(super) struct RelayTransportMap {
    rows: BTreeMap<CanonicalRelayUrl, RelayTransportStatus>,
    /// ADR-0051 — per-URL relay-information documents (NIP-11), keyed by the
    /// SAME canonical URL as `rows`. Stored independently of the role-keyed
    /// transport row so the `nmp-nip11` fetch result (which has no role) can
    /// attach a document to any connected URL, and so the doc survives the
    /// transient role bookkeeping. Surfaced on `RelayStatus.info`.
    info: BTreeMap<CanonicalRelayUrl, InfoEntry>,
}

/// A cached relay-information document plus the monotonic instant it was last
/// fetched, for TTL gating (`is_info_fresh`).
#[derive(Clone, Debug)]
struct InfoEntry {
    doc: RelayInfoDoc,
    /// Only read by `info_is_fresh`, which is `#[cfg(test)]`-guarded.
    #[allow(dead_code)]
    fetched_at: Instant,
}

#[derive(Clone, Debug)]
struct RelayTransportStatus {
    role: RelayRole,
    connection: String,
    auth: String,
    connected_at: Option<Instant>,
    last_event_at: Option<Instant>,
    last_notice: Option<String>,
    /// Bounded NOTICE log (oldest first). Mirrored from `RelayHealth.notices`
    /// for the URL-keyed transport path; populated in `record_transport_notice`.
    notices: VecDeque<NoticeEntry>,
    last_error: Option<String>,
    error_category: Option<String>,
    reconnect_count: u32,
    counters: Counters,
    denied: bool,
    last_close_reason: Option<String>,
}

impl RelayTransportStatus {
    fn new(role: RelayRole) -> Self {
        Self {
            role,
            connection: "unknown".to_string(),
            auth: "not_required".to_string(),
            connected_at: None,
            last_event_at: None,
            last_notice: None,
            notices: VecDeque::new(),
            last_error: None,
            error_category: None,
            reconnect_count: 0,
            counters: Counters::default(),
            denied: false,
            last_close_reason: None,
        }
    }
}

impl RelayTransportMap {
    pub(super) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    fn entry(&mut self, role: RelayRole, relay_url: &str) -> &mut RelayTransportStatus {
        let key = CanonicalRelayUrl::parse_or_raw(relay_url);
        self.rows
            .entry(key)
            .or_insert_with(|| RelayTransportStatus::new(role))
    }

    /// B3 — is any indexer-role socket currently connected?
    ///
    /// Scans the per-URL transport rows for an `RelayRole::Indexer` row in the
    /// `"connected"` state. The kernel feeds the answer to
    /// [`crate::subs::SubscriptionLifecycle::note_indexer_lane_recovered`] after
    /// every indexer connect / fail / close so the mailbox-probe epoch advances
    /// on a genuine outage recovery (and only then). A URL whose role rotates
    /// is keyed by its latest role, which is sufficient here — the indexer lane
    /// is "up" iff ≥1 of these rows reports connected.
    fn any_indexer_connected(&self) -> bool {
        self.rows
            .values()
            .any(|row| row.role == RelayRole::Indexer && row.connection == "connected")
    }

    fn statuses(&self, kernel: &Kernel) -> BTreeMap<String, RelayStatus> {
        self.rows
            .iter()
            .map(|(url, row)| {
                (
                    url.to_string(),
                    RelayStatus {
                        role: row.role.key().to_string(),
                        relay_url: url.to_string(),
                        connection: row.connection.clone(),
                        auth: row.auth.clone(),
                        negentropy_probe: kernel.relay(row.role).negentropy_probe_state.clone(),
                        active_wire_subscriptions: active_wire_subscriptions(
                            &kernel.wire.subs,
                            url,
                        ),
                        reconnect_count: row.reconnect_count,
                        last_connected_at_ms: kernel.elapsed_ms(row.connected_at),
                        last_event_at_ms: kernel.elapsed_ms(row.last_event_at),
                        last_notice: row.last_notice.clone(),
                        last_error: row.last_error.clone(),
                        error_category: row.error_category.clone(),
                        events_rx: row.counters.events_rx,
                        notices_rx: row.counters.notices_rx,
                        notices: row.notices.iter().cloned().collect(),
                        bytes_rx: row.counters.bytes_rx,
                        bytes_tx: row.counters.bytes_tx,
                        denied: row.denied,
                        last_close_reason: row.last_close_reason.clone(),
                        info: self.info.get(url).map(|e| e.doc.clone()),
                    },
                )
            })
            .collect()
    }

    /// Store / replace the relay-information document for `relay_url`,
    /// anchoring its freshness to `now` (ADR-0051).
    fn set_info(&mut self, relay_url: &str, doc: RelayInfoDoc, now: Instant) {
        let key = CanonicalRelayUrl::parse_or_raw(relay_url);
        self.info.insert(
            key,
            InfoEntry {
                doc,
                fetched_at: now,
            },
        );
    }

    /// Whether a *fresh* (within `ttl`) document already exists for
    /// `relay_url` as of `now`. Only called from `relay_info_is_fresh`.
    #[cfg(test)] // consumed by relay_diagnostics/tests.rs via relay_info_is_fresh
    fn info_is_fresh(&self, relay_url: &str, now: Instant, ttl: std::time::Duration) -> bool {
        let key = CanonicalRelayUrl::parse_or_raw(relay_url);
        self.info
            .get(&key)
            .is_some_and(|e| now.saturating_duration_since(e.fetched_at) < ttl)
    }

    /// Read the stored document for `relay_url`, if any.
    fn info_for(&self, relay_url: &str) -> Option<&RelayInfoDoc> {
        let key = CanonicalRelayUrl::parse_or_raw(relay_url);
        self.info.get(&key).map(|e| &e.doc)
    }
}

impl Kernel {
    pub(crate) fn relay_socket_is_persistent(
        &self,
        relay_url: &CanonicalRelayUrl,
        role: RelayRole,
    ) -> bool {
        // Both the NWC wallet lane and the NIP-46 signer lane are on-demand
        // persistent sockets — they must never be reaped by the idle sweeper
        // while the session is live. All other roles fall through to the
        // bootstrap/configured-relay check below.
        if role == RelayRole::Wallet || role == RelayRole::Signer {
            return true;
        }
        RelayRole::all()
            .into_iter()
            .flat_map(|role| self.bootstrap_urls_for_role(role))
            .any(|url| CanonicalRelayUrl::parse_or_raw(&url) == *relay_url)
            || self
                .configured_relays
                .iter()
                .any(|row| CanonicalRelayUrl::parse_or_raw(&row.url) == *relay_url)
    }

    /// Test-support accessor: check persistence by raw URL string + role.
    ///
    /// Wraps the `pub(crate)` `relay_socket_is_persistent` behind the
    /// `test-support` feature gate so external crates (e.g. `nmp-nip46-runtime`)
    /// can verify the relay-lifetime contract without promoting the main
    /// function to `pub`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn relay_socket_is_persistent_for_test(&self, relay_url: &str, role: RelayRole) -> bool {
        let canonical = CanonicalRelayUrl::parse_or_raw(relay_url);
        self.relay_socket_is_persistent(&canonical, role)
    }

    pub(crate) fn relay_has_active_demand(&self, relay_url: &CanonicalRelayUrl) -> bool {
        self.wire.subs.values().any(|sub| {
            sub.relay_url == *relay_url
                && !matches!(sub.state.as_str(), "closed" | "closed_by_relay")
        }) || self
            .deferred_outbound
            .iter()
            .any(|message| CanonicalRelayUrl::parse_or_raw(&message.relay_url) == *relay_url)
            || self.publish_engine.has_active_relay(relay_url.as_str())
    }

    pub(super) fn relay_diagnostics_statuses(&self) -> Vec<RelayStatus> {
        if self.transport_relays.is_empty() {
            return self.relay_statuses();
        }

        let mut by_url = self.transport_relays.statuses(self);
        let mut ordered = Vec::with_capacity(by_url.len());
        let mut emitted = HashSet::new();
        for role in RelayRole::all() {
            for relay_url in self.bootstrap_urls_for_role(role) {
                let key = CanonicalRelayUrl::parse_or_raw(&relay_url).into_string();
                if let Some(status) = by_url.remove(&key) {
                    emitted.insert(key);
                    ordered.push(status);
                }
            }
        }
        ordered.extend(
            by_url
                .into_iter()
                .filter_map(|(url, status)| emitted.insert(url).then_some(status)),
        );
        ordered
    }

    /// ADR-0051 — fold a fetched relay-information document onto the per-URL
    /// transport row and mark the snapshot dirty so the `relay_diagnostics`
    /// projection surfaces the new metadata on the next emit. Called from the
    /// actor's [`crate::ActorCommand::SetRelayInfo`] dispatch arm.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_relay_info(&mut self, relay_url: &str, doc: RelayInfoDoc) {
        self.set_relay_info_at(
            relay_url,
            doc,
            crate::kernel::test_support::test_support_now(),
        );
    }

    pub(crate) fn set_relay_info_at(&mut self, relay_url: &str, doc: RelayInfoDoc, now: Instant) {
        self.transport_relays.set_info(relay_url, doc, now);
        self.changed_since_emit = true;
    }

    /// ADR-0051 — whether a fresh (within `ttl`) relay-information document
    /// already exists for `relay_url`. Only used by relay_diagnostics tests.
    #[cfg(test)] // consumed by relay_diagnostics/tests.rs
    #[must_use]
    pub(crate) fn relay_info_is_fresh(&self, relay_url: &str, ttl: std::time::Duration) -> bool {
        self.transport_relays
            .info_is_fresh(relay_url, Instant::now(), ttl) // doctrine-allow: D9 — cfg(test) relay-info freshness helper
    }

    /// ADR-0051 — read the cached relay-information document for `relay_url`.
    #[must_use]
    pub(crate) fn relay_info_for(&self, relay_url: &str) -> Option<&RelayInfoDoc> {
        self.transport_relays.info_for(relay_url)
    }

    pub(crate) fn record_tx_to(&mut self, role: RelayRole, relay_url: &str, bytes: usize) {
        self.record_tx(role, bytes);
        let entry = self.transport_relays.entry(role, relay_url);
        entry.counters.bytes_tx = entry.counters.bytes_tx.saturating_add(bytes as u64);
    }

    pub(super) fn mark_transport_connecting(&mut self, role: RelayRole, relay_url: &str) {
        let entry = self.transport_relays.entry(role, relay_url);
        entry.connection = "connecting".to_string();
        entry.last_error = None;
        entry.error_category = None;
    }

    pub(super) fn mark_transport_connected(&mut self, role: RelayRole, relay_url: &str) {
        let entry = self.transport_relays.entry(role, relay_url);
        entry.connection = "connected".to_string();
        entry.connected_at = Some(Instant::now()); // doctrine-allow: D9 — transport diagnostic elapsed-time marker; not replay policy
        entry.last_error = None;
        entry.error_category = None;
        entry.auth = "not_required".to_string();
        entry.denied = false;
        entry.last_close_reason = None;
    }

    pub(super) fn mark_transport_failed(
        &mut self,
        role: RelayRole,
        relay_url: &str,
        error: String,
    ) {
        let entry = self.transport_relays.entry(role, relay_url);
        entry.connection = "backing_off".to_string();
        entry.last_error = Some(super::truncate(&error, 160));
        entry.error_category = Some(super::closed_reason::ERR_TRANSIENT.to_string());
        entry.reconnect_count = entry.reconnect_count.saturating_add(1);
    }

    pub(super) fn mark_transport_closed(&mut self, role: RelayRole, relay_url: &str) {
        let entry = self.transport_relays.entry(role, relay_url);
        entry.connection = "closed".to_string();
        entry.auth = "not_required".to_string();
    }

    pub(super) fn mark_transport_role_closed(&mut self, role: RelayRole) {
        for row in self
            .transport_relays
            .rows
            .values_mut()
            .filter(|row| row.role == role)
        {
            row.connection = "closed".to_string();
            row.auth = "not_required".to_string();
        }
    }

    /// B3 — feed the current indexer-lane connectivity to the subscription
    /// lifecycle so it can advance the mailbox-probe epoch on a genuine outage
    /// recovery (and re-arm `probed_mailboxes`).
    ///
    /// Called from the per-URL indexer transition sites (`relay_connected_url`,
    /// `relay_failed`, `relay_closed`, `relay_closed_all`) AFTER the transport
    /// row's `connection` state is updated. Cheap (`any_indexer_connected` is an
    /// O(rows) scan and the lifecycle gate is a single bool edge), so calling it
    /// on every indexer transition is fine; it is a no-op while the lane stays
    /// up. No-op for non-indexer transitions (the lane truth is unchanged).
    ///
    /// D4: the lifecycle remains the single owner of the probed set / epoch —
    /// the kernel only reports the OS-level connectivity edge (D7).
    pub(super) fn observe_indexer_lane_health(&mut self) {
        let any_indexer_connected = self.transport_relays.any_indexer_connected();
        let _re_armed = self
            .lifecycle
            .note_indexer_lane_recovered(any_indexer_connected);
    }

    pub(super) fn record_transport_rx(&mut self, role: RelayRole, relay_url: &str, bytes: usize) {
        let entry = self.transport_relays.entry(role, relay_url);
        entry.counters.frames_rx = entry.counters.frames_rx.saturating_add(1);
        entry.counters.bytes_rx = entry.counters.bytes_rx.saturating_add(bytes as u64);
    }

    pub(super) fn record_transport_event(&mut self, role: RelayRole, relay_url: &str, at: Instant) {
        let entry = self.transport_relays.entry(role, relay_url);
        entry.counters.events_rx = entry.counters.events_rx.saturating_add(1);
        entry.last_event_at = Some(at);
    }

    pub(super) fn record_transport_eose(&mut self, role: RelayRole, relay_url: &str) {
        let entry = self.transport_relays.entry(role, relay_url);
        entry.counters.eose_rx = entry.counters.eose_rx.saturating_add(1);
    }

    pub(super) fn record_transport_notice(
        &mut self,
        role: RelayRole,
        relay_url: &str,
        notice: String,
    ) {
        let at_ms = self.now_ms();
        let entry = self.transport_relays.entry(role, relay_url);
        entry.counters.notices_rx = entry.counters.notices_rx.saturating_add(1);
        entry.last_notice = Some(notice.clone());
        if entry.notices.len() >= MAX_NOTICE_LOG {
            entry.notices.pop_front();
        }
        entry.notices.push_back(NoticeEntry {
            at_ms,
            text: notice,
        });
    }

    pub(super) fn record_transport_closed_frame(&mut self, role: RelayRole, relay_url: &str) {
        let entry = self.transport_relays.entry(role, relay_url);
        entry.counters.closed_rx = entry.counters.closed_rx.saturating_add(1);
    }

    pub(super) fn sync_transport_from_lane(&mut self, role: RelayRole, relay_url: &str) {
        let relay = self.relay(role).clone();
        let entry = self.transport_relays.entry(role, relay_url);
        entry.auth = relay.auth;
        entry.last_error = relay.last_error;
        entry.error_category = relay.error_category;
        entry.denied = relay.denied;
        entry.last_close_reason = relay.last_close_reason;
    }
}

fn active_wire_subscriptions(
    subs: &std::collections::HashMap<(CanonicalRelayUrl, String), WireSub>,
    relay_url: &CanonicalRelayUrl,
) -> usize {
    subs.values()
        .filter(|sub| {
            &sub.relay_url == relay_url
                && !matches!(sub.state.as_str(), "closed" | "closed_by_relay")
        })
        .count()
}
