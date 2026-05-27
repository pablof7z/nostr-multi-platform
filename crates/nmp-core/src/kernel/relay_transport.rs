//! URL-level relay transport diagnostics.
//!
//! `RelayHealth` is role-lane state (`content`, `indexer`, `wallet`). The
//! native/browser transport pools are URL-keyed, so diagnostics need a second
//! projection that keeps one row per actual socket URL while preserving the
//! legacy `RelayStatus` shape consumed by shells.

use std::collections::{BTreeMap, HashSet};

use super::{CanonicalRelayUrl, Counters, Instant, Kernel, RelayRole, RelayStatus, WireSub};

#[derive(Clone, Debug, Default)]
pub(super) struct RelayTransportMap {
    rows: BTreeMap<CanonicalRelayUrl, RelayTransportStatus>,
}

#[derive(Clone, Debug)]
struct RelayTransportStatus {
    role: RelayRole,
    connection: String,
    auth: String,
    connected_at: Option<Instant>,
    last_event_at: Option<Instant>,
    last_notice: Option<String>,
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
                        bytes_rx: row.counters.bytes_rx,
                        bytes_tx: row.counters.bytes_tx,
                        denied: row.denied,
                        last_close_reason: row.last_close_reason.clone(),
                    },
                )
            })
            .collect()
    }
}

impl Kernel {
    pub(crate) fn relay_socket_is_persistent(
        &self,
        relay_url: &CanonicalRelayUrl,
        role: RelayRole,
    ) -> bool {
        if role == RelayRole::Wallet {
            return true;
        }
        RelayRole::all()
            .into_iter()
            .flat_map(|role| self.bootstrap_urls_for_role(role))
            .any(|url| CanonicalRelayUrl::parse_or_raw(&url) == *relay_url)
            || self
                .relay_edit_rows
                .iter()
                .any(|row| CanonicalRelayUrl::parse_or_raw(&row.url) == *relay_url)
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
        entry.connected_at = Some(Instant::now());
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
        let entry = self.transport_relays.entry(role, relay_url);
        entry.counters.notices_rx = entry.counters.notices_rx.saturating_add(1);
        entry.last_notice = Some(notice);
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
