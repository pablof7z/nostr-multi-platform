//! Live relay-frame parsing and relay-specific accepted-event bookkeeping.

use super::super::{
    truncate, CanonicalRelayUrl, Instant, Kernel, NostrEvent, NoticeEntry, OutboundMessage,
    RelayFrame, RelayRole, Value, MAX_NOTICE_LOG,
};
use super::IngestSource;

impl Kernel {
    /// Ingest a single inbound relay frame on the named role/url.
    ///
    /// V-01 Phase 1c: takes [`RelayFrame`] (a wire-transport-agnostic enum)
    /// rather than `tungstenite::Message` directly. The native
    /// `relay_worker` converts each `tungstenite::Message` to a
    /// [`RelayFrame`] before calling this; a non-native transport (wasm32
    /// WebSocket) is responsible for its own equivalent conversion. The
    /// kernel itself never names `tungstenite`.
    #[cfg(test)]
    pub(crate) fn handle_message(
        &mut self,
        role: RelayRole,
        relay_url: &str,
        message: RelayFrame,
    ) -> Vec<OutboundMessage> {
        self.handle_message_at(
            role,
            relay_url,
            message,
            crate::kernel::test_support::test_support_now(),
        )
    }

    pub(crate) fn handle_message_at(
        &mut self,
        role: RelayRole,
        relay_url: &str,
        message: RelayFrame,
        now: Instant,
    ) -> Vec<OutboundMessage> {
        match message {
            RelayFrame::Text(text) => {
                let relay = self.relay_mut(role);
                relay.counters.frames_rx = relay.counters.frames_rx.saturating_add(1);
                relay.counters.bytes_rx = relay.counters.bytes_rx.saturating_add(text.len() as u64);
                self.record_transport_rx(role, relay_url, text.len());
                let mut outbound = self.handle_text_at(role, relay_url, &text, now);
                outbound.extend(self.tick_publish_engine_for_now());
                outbound
            }
            RelayFrame::Binary(bytes) => {
                let relay = self.relay_mut(role);
                relay.counters.frames_rx = relay.counters.frames_rx.saturating_add(1);
                relay.counters.bytes_rx =
                    relay.counters.bytes_rx.saturating_add(bytes.len() as u64);
                self.record_transport_rx(role, relay_url, bytes.len());
                Vec::new()
            }
            RelayFrame::Ping | RelayFrame::Pong => Vec::new(),
            RelayFrame::Close(reason) => {
                let relay = self.relay_mut(role);
                relay.connection = "closed".to_string();
                relay.last_error = reason;
                self.mark_transport_closed(role, relay_url);
                self.sync_transport_from_lane(role, relay_url);
                self.changed_since_emit = true;
                Vec::new()
            }
        }
    }

    #[cfg(test)]
    pub(in crate::kernel) fn handle_text(
        &mut self,
        role: RelayRole,
        relay_url: &str,
        text: &str,
    ) -> Vec<OutboundMessage> {
        self.handle_text_at(
            role,
            relay_url,
            text,
            crate::kernel::test_support::test_support_now(),
        )
    }

    pub(in crate::kernel) fn handle_text_at(
        &mut self,
        role: RelayRole,
        relay_url: &str,
        text: &str,
        now: Instant,
    ) -> Vec<OutboundMessage> {
        // Canonicalize only the map key. Auth/publish/closed policy uses the
        // delivering URL because NIP-42 replay protection is URL-specific.
        let wire_key_url = CanonicalRelayUrl::parse_or_raw(relay_url);
        let Ok(value) = serde_json::from_str::<Value>(text) else {
            self.log(format!("unparseable relay frame: {}", truncate(text, 120)));
            return Vec::new();
        };

        let Some(array) = value.as_array() else {
            return Vec::new();
        };

        let Some(kind) = array.first().and_then(Value::as_str) else {
            return Vec::new();
        };

        let mut outbound = Vec::new();
        match kind {
            "EVENT" => {
                let sub_id = array.get(1).and_then(Value::as_str).unwrap_or("unknown");
                if let Some(event_value) = array.get(2) {
                    outbound.extend(self.handle_event(role, relay_url, sub_id, event_value));
                }
            }
            "EOSE" => {
                let sub_id = array.get(1).and_then(Value::as_str).unwrap_or("unknown");
                self.handle_eose(role, relay_url, sub_id, &wire_key_url, &mut outbound);
            }
            "NOTICE" => {
                let notice = array
                    .get(1)
                    .and_then(Value::as_str)
                    .map_or_else(|| "notice".to_string(), |s| truncate(s, 180));
                // Capture timestamp before mutable borrow (NLL: &self ends here).
                let at_ms = self.now_ms();
                let relay = self.relay_mut(role);
                relay.counters.notices_rx = relay.counters.notices_rx.saturating_add(1);
                relay.last_notice = Some(notice.clone());
                if relay.notices.len() >= MAX_NOTICE_LOG {
                    relay.notices.pop_front();
                }
                relay.notices.push_back(NoticeEntry {
                    at_ms,
                    text: notice.clone(),
                });
                // relay borrow ends; transport map updated separately (URL-keyed).
                self.record_transport_notice(role, relay_url, notice.clone());
                self.changed_since_emit = true;
                self.log(format!("NOTICE {} {notice}", role.key()));
            }
            "CLOSED" => {
                let sub_id = array
                    .get(1)
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_string();
                let reason = array
                    .get(2)
                    .and_then(Value::as_str)
                    .map(|s| truncate(s, 180));
                {
                    let relay = self.relay_mut(role);
                    relay.counters.closed_rx = relay.counters.closed_rx.saturating_add(1);
                }
                self.record_transport_closed_frame(role, relay_url);
                self.wire
                    .subs
                    .remove(&(wire_key_url.clone(), sub_id.clone()));
                self.changed_since_emit = true;
                self.classify_and_route_closed(role, relay_url, &sub_id, reason.as_deref());
                self.sync_transport_from_lane(role, relay_url);
            }
            "OK" => {
                outbound.extend(self.handle_auth_ok(role, relay_url, array));
                outbound.extend(self.route_publish_ok(relay_url, array));
            }
            "AUTH" => {
                outbound.extend(self.handle_auth_challenge(role, relay_url, array));
            }
            _ => self.log(format!("relay frame {kind}")),
        }

        outbound.extend(self.maybe_open_timeline_at(now));
        outbound
    }

    pub(in crate::kernel) fn handle_event(
        &mut self,
        role: RelayRole,
        relay_url: &str,
        sub_id: &str,
        value: &Value,
    ) -> Vec<OutboundMessage> {
        let Ok(event) = serde_json::from_value::<NostrEvent>(value.clone()) else {
            self.log(format!("bad EVENT payload on {sub_id}"));
            return Vec::new();
        };

        let now = Instant::now(); // doctrine-allow: D9 — relay/event diagnostic elapsed-time marker; not replay policy
        {
            let relay = self.relay_mut(role);
            relay.counters.events_rx = relay.counters.events_rx.saturating_add(1);
            relay.last_event_at = Some(now);
        }
        self.record_transport_event(role, relay_url, now);
        self.events_since_last_update = self.events_since_last_update.saturating_add(1);
        self.timing.last_event_at = Some(now);
        self.timing.first_event_at.get_or_insert(now);
        let wire_key_url = CanonicalRelayUrl::parse_or_raw(relay_url);
        if let Some(sub) = self.wire.subs.get_mut(&(wire_key_url, sub_id.to_string())) {
            if sub.state == "opening" {
                sub.state = "live".to_string();
            }
            sub.events_rx = sub.events_rx.saturating_add(1);
            sub.last_event_at = Some(now);
        }
        let claim_match_author = self.claim_expansion_match_author(sub_id, &event);
        let event_id_for_score = event.id.clone();

        let outcome = self.ingest_accepted_event(IngestSource::Relay { relay_url, sub_id }, event);

        let mut outbound = Vec::new();
        if matches!(
            outcome,
            Some(
                crate::store::InsertOutcome::Inserted { .. }
                    | crate::store::InsertOutcome::Replaced { .. }
                    | crate::store::InsertOutcome::Duplicate { .. }
                    | crate::store::InsertOutcome::Ephemeral { .. }
            )
        ) {
            outbound.extend(self.handle_publish_event_echo(relay_url, &event_id_for_score));
        }

        if let Some(author) = claim_match_author.as_deref() {
            if matches!(
                outcome,
                Some(
                    crate::store::InsertOutcome::Inserted { .. }
                        | crate::store::InsertOutcome::Replaced { .. }
                )
            ) {
                self.record_claim_expansion_hit(sub_id, relay_url, author, &event_id_for_score);
            }
        }
        self.changed_since_emit = true;
        outbound
    }
}
