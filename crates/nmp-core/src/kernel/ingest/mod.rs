//! Relay-frame parsing and event-kind dispatch.
//!
//! `handle_message` → `handle_text` → `handle_event` → kind-specific ingest:
//! - kind:0  → `profile.rs` (`ingest_profile`)
//! - kind:3  → `contacts.rs` (`ingest_contacts`)
//! - kind:10002` → `relay_list.rs` (`ingest_relay_list`)
//! - kind:1|6 → `timeline.rs` (`ingest_timeline_event`)
//!
//! `verify_and_persist` is the shared store-insertion path for non-timeline kinds.

mod auth_handlers;
mod contacts;
mod profile;
mod relay_list;
mod timeline;

use super::*;

/// Returns up to the first 16 chars of an event id, safe for any length.
fn event_short_id(id: &str) -> &str {
    &id[..id.len().min(16)]
}

impl Kernel {
    pub(crate) fn handle_message(
        &mut self,
        role: RelayRole,
        message: Message,
    ) -> Vec<OutboundMessage> {
        match message {
            Message::Text(text) => {
                let relay = self.relay_mut(role);
                relay.counters.frames_rx = relay.counters.frames_rx.saturating_add(1);
                relay.counters.bytes_rx = relay.counters.bytes_rx.saturating_add(text.len() as u64);
                self.handle_text(role, &text)
            }
            Message::Binary(bytes) => {
                let relay = self.relay_mut(role);
                relay.counters.frames_rx = relay.counters.frames_rx.saturating_add(1);
                relay.counters.bytes_rx =
                    relay.counters.bytes_rx.saturating_add(bytes.len() as u64);
                Vec::new()
            }
            Message::Ping(_) | Message::Pong(_) => Vec::new(),
            Message::Close(frame) => {
                let relay = self.relay_mut(role);
                relay.connection = "closed".to_string();
                relay.last_error = frame.map(|frame| frame.reason.to_string());
                self.changed_since_emit = true;
                Vec::new()
            }
            Message::Frame(_) => Vec::new(),
        }
    }

    pub(super) fn handle_text(&mut self, role: RelayRole, text: &str) -> Vec<OutboundMessage> {
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
                    self.handle_event(role, sub_id, event_value);
                }
            }
            "EOSE" => {
                let sub_id = array.get(1).and_then(Value::as_str).unwrap_or("unknown");
                {
                    let relay = self.relay_mut(role);
                    relay.counters.eose_rx = relay.counters.eose_rx.saturating_add(1);
                }
                if let Some(sub) = self.wire_subs.get_mut(sub_id) {
                    sub.state = if sub_id == "seed-timeline" || sub_id.starts_with("diag-firehose-")
                    {
                        "live".to_string()
                    } else {
                        "closed".to_string()
                    };
                    sub.eose_at = Some(Instant::now());
                }
                if sub_id.starts_with("thread-ids-") {
                    self.thread_ids_inflight = false;
                }
                if sub_id.starts_with("thread-replies-") {
                    self.thread_replies_inflight = false;
                }
                // T82: a discovery oneshot's first stored set has landed
                // (OneShot lifecycle == "EOSE closes"). Complete + release the
                // token; the generic CLOSE below tears down the wire sub.
                if sub_id.starts_with(crate::kernel::discovery::ONESHOT_SUB_PREFIX) {
                    self.complete_unknown_oneshot(sub_id);
                }
                if sub_id != "seed-timeline" && !sub_id.starts_with("diag-firehose-") {
                    // T105: CLOSE must travel back to the same socket the REQ
                    // went out on — the transport pool is URL-keyed, so a
                    // role-only close would target the bootstrap socket and
                    // leave the resolved sub open. Pull the recorded URL from
                    // the WireSub set on req_for_relay; fall back to the
                    // delivering relay's URL when the sub_id is unknown.
                    let relay_url = self
                        .wire_subs
                        .get(sub_id)
                        .map(|sub| sub.relay_url.clone())
                        .unwrap_or_else(|| role.url().to_string());
                    outbound.push(OutboundMessage {
                        role,
                        relay_url,
                        text: json!(["CLOSE", sub_id]).to_string(),
                    });
                }
                self.changed_since_emit = true;
                self.log(format!("EOSE {sub_id}"));
            }
            "NOTICE" => {
                let notice = array
                    .get(1)
                    .and_then(Value::as_str)
                    .map(|s| truncate(s, 180))
                    .unwrap_or_else(|| "notice".to_string());
                let relay = self.relay_mut(role);
                relay.counters.notices_rx = relay.counters.notices_rx.saturating_add(1);
                relay.last_notice = Some(notice.clone());
                self.changed_since_emit = true;
                self.log(format!("NOTICE {} {notice}", role.key()));
            }
            "CLOSED" => {
                let sub_id = array.get(1).and_then(Value::as_str).unwrap_or("unknown");
                let reason = array
                    .get(2)
                    .and_then(Value::as_str)
                    .map(|s| truncate(s, 180));
                {
                    let relay = self.relay_mut(role);
                    relay.counters.closed_rx = relay.counters.closed_rx.saturating_add(1);
                }
                if let Some(sub) = self.wire_subs.get_mut(sub_id) {
                    sub.state = "closed_by_relay".to_string();
                    sub.close_reason = reason.clone();
                }
                if sub_id.starts_with("thread-ids-") {
                    self.thread_ids_inflight = false;
                }
                if sub_id.starts_with("thread-replies-") {
                    self.thread_replies_inflight = false;
                }
                self.changed_since_emit = true;
                self.log(format!(
                    "CLOSED {sub_id} {}",
                    reason.unwrap_or_else(|| "".to_string())
                ));
            }
            "OK" => {
                // M5+M2+M8 wiring: an OK frame may be the ack of an in-flight
                // kind:22242. Non-AUTH OKs (publish acks etc.) are no-ops here;
                // the publish engine has its own OK matcher per `nmp-core::publish`.
                outbound.extend(self.handle_auth_ok(role, array));
            }
            "AUTH" => {
                // M5+M2+M8 wiring: relay-initiated NIP-42 challenge. Builds the
                // kind:22242 via the bound signer (if any) and fans the new
                // RelayAuthState into the lifecycle's AuthGate so future REQs
                // to this relay are buffered until `Authenticated`. AUTH-state
                // transitions never set `changed_since_emit` — D8 invariant.
                outbound.extend(self.handle_auth_challenge(role, array));
            }
            _ => self.log(format!("relay frame {kind}")),
        }

        outbound.extend(self.maybe_open_timeline());
        outbound.extend(self.maybe_open_thread_hydration());
        // M5+M2+M8 wiring: the AUTH-pause partition lives at the single
        // send-time choke point in `actor::relay_mgmt::send_all_outbound`, so
        // every REQ regardless of producer (handle_text, view-open commands,
        // startup, pending) is screened uniformly. No partition needed here.
        outbound
    }

    pub(super) fn handle_event(&mut self, role: RelayRole, sub_id: &str, value: &Value) {
        let Ok(event) = serde_json::from_value::<NostrEvent>(value.clone()) else {
            self.log(format!("bad EVENT payload on {sub_id}"));
            return;
        };

        let now = Instant::now();
        {
            let relay = self.relay_mut(role);
            relay.counters.events_rx = relay.counters.events_rx.saturating_add(1);
            relay.last_event_at = Some(now);
        }
        self.events_since_last_update = self.events_since_last_update.saturating_add(1);
        self.last_event_at = Some(now);
        self.first_event_at.get_or_insert(now);
        if let Some(sub) = self.wire_subs.get_mut(sub_id) {
            if sub.state == "opening" {
                sub.state = "live".to_string();
            }
            sub.last_event_at = Some(now);
        }

        // D4: all events are persisted before kind-specific dispatch.
        // Kinds 1|6 handle their own store.insert inside ingest_timeline_event.
        // For replaceable kinds (0, 3, 10002) we gate local cache mutations on
        // the store outcome: only Inserted | Replaced means this event is now
        // canonical (D4).
        match event.kind {
            1 | 6 => self.ingest_timeline_event(role, sub_id, event),
            0 => {
                use crate::store::InsertOutcome;
                let outcome = self.verify_and_persist(role, &event);
                if matches!(
                    outcome,
                    Some(InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. })
                ) {
                    self.ingest_profile(event);
                }
                self.changed_since_emit = true;
            }
            3 => {
                use crate::store::InsertOutcome;
                let outcome = self.verify_and_persist(role, &event);
                if matches!(
                    outcome,
                    Some(InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. })
                ) {
                    self.ingest_contacts(event);
                }
                self.changed_since_emit = true;
            }
            10002 => {
                use crate::store::InsertOutcome;
                let outcome = self.verify_and_persist(role, &event);
                if matches!(
                    outcome,
                    Some(InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. })
                ) {
                    self.ingest_relay_list(event);
                }
                self.changed_since_emit = true;
            }
            _ => {
                self.verify_and_persist(role, &event);
                self.changed_since_emit = true;
            }
        }
    }

    /// Verify and persist an event to the EventStore.
    ///
    /// Returns `Some(outcome)` with the store's [`InsertOutcome`] when
    /// verification succeeds, or `None` when signature verification fails.
    /// Callers that perform local-cache mutations for replaceable kinds **must**
    /// inspect the outcome: only `Inserted | Replaced` means this event is now
    /// the canonical version in the store — all other outcomes must be treated
    /// as no-ops for cache purposes (D4).
    pub(super) fn verify_and_persist(
        &mut self,
        role: RelayRole,
        event: &NostrEvent,
    ) -> Option<crate::store::InsertOutcome> {
        let raw = crate::store::RawEvent {
            id: event.id.clone(),
            pubkey: event.pubkey.clone(),
            created_at: event.created_at,
            kind: event.kind,
            tags: event.tags.clone(),
            content: event.content.clone(),
            sig: event.sig.clone(),
        };
        let verified = match crate::store::VerifiedEvent::try_from_raw(raw) {
            Ok(v) => v,
            Err(e) => {
                self.log(format!(
                    "sig verify failed for {}: {e}",
                    event_short_id(&event.id)
                ));
                return None;
            }
        };
        let relay_url = role.url().to_string();
        let received_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        match self.store.insert(verified, &relay_url, received_at_ms) {
            Ok(outcome) => Some(outcome),
            Err(e) => {
                self.log(format!(
                    "store insert error for {}: {e}",
                    event_short_id(&event.id)
                ));
                None
            }
        }
    }
}
