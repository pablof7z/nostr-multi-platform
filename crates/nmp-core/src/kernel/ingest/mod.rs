//! Relay-frame parsing and event-kind dispatch.
//!
//! `handle_message` → `handle_text` → `handle_event` → kind-specific ingest:
//! - kind:0  → `profile.rs` (`ingest_profile`)
//! - kind:3  → `contacts.rs` (`ingest_contacts`)
//! - kind:1|6 → `timeline.rs` (`ingest_timeline_event`)
//!
//! Every other kind (kind:10002 NIP-65 mailbox lists, kind:10050 NIP-17
//! DM-relay lists, future NIP-51 lists, …) routes through the substrate
//! [`crate::substrate::EventIngestDispatcher`] — the wildcard arm fans
//! the [`crate::store::VerifiedEvent`] to every registered
//! [`crate::substrate::IngestParser`] before the `KernelEventObserver`s
//! fire. Per-NIP crates register their parsers at composition time; the
//! kernel never names the NIP kind directly.
//!
//! For parsers that mutate the substrate
//! [`crate::substrate::MailboxCache`], the wildcard arm also observes the
//! cache state for the event author before/after dispatch — when the cache
//! transitioned the kernel fires the `route_subscription_relays` trace
//! observer and enqueues the `Nip65Arrived` recompile trigger, both
//! kind-agnostically (the kernel only knows "the mailbox cache changed
//! for this author", not "a kind:10002 arrived"). This replaces the
//! pre-2026-05-25 `match event.kind { 10002 => ... }` arm + the deleted
//! `relay_list.rs` impl, which both named NIP-65 explicitly and were a
//! D0 violation (`docs/architecture/crate-boundaries.md` §0).
//!
//! `verify_and_persist` is the shared store-insertion path for non-timeline kinds.

mod auth_handlers;
mod closed;
mod contacts;
mod eose;
mod event;
mod profile;
mod timeline;
mod timeline_order;

use super::{
    truncate, CanonicalRelayUrl, Kernel, NostrEvent, OutboundMessage, RelayFrame, RelayRole, Value,
};

/// Returns up to the first 16 chars of an event id, safe for any length.
fn event_short_id(id: &str) -> &str {
    &id[..id.len().min(16)]
}

/// Project a wire-parsed [`NostrEvent`] into the store's [`crate::store::RawEvent`].
///
/// The signed-event tap, `verify_and_persist`, and `ingest_timeline_event`
/// each need an identical `RawEvent` to feed `VerifiedEvent::try_from_raw` —
/// this is the single construction site so the field list never drifts.
fn raw_event_from_nostr(event: &NostrEvent) -> crate::store::RawEvent {
    crate::store::RawEvent {
        id: event.id.clone(),
        pubkey: event.pubkey.clone(),
        created_at: event.created_at,
        kind: event.kind,
        tags: event.tags.clone(),
        content: event.content.clone(),
        sig: event.sig.clone(),
    }
}

pub(super) fn raw_tap_should_fire(outcome: &crate::store::InsertOutcome) -> bool {
    use crate::store::InsertOutcome;
    matches!(
        outcome,
        InsertOutcome::Inserted { .. }
            | InsertOutcome::Duplicate { .. }
            | InsertOutcome::Replaced { .. }
            | InsertOutcome::Ephemeral { .. }
    )
}

impl Kernel {
    /// Ingest a single inbound relay frame on the named role/url.
    ///
    /// V-01 Phase 1c: takes [`RelayFrame`] (a wire-transport-agnostic enum)
    /// rather than `tungstenite::Message` directly. The native
    /// `relay_worker` converts each `tungstenite::Message` to a
    /// [`RelayFrame`] before calling this; a non-native transport (wasm32
    /// WebSocket) is responsible for its own equivalent conversion. The
    /// kernel itself never names `tungstenite`.
    pub(crate) fn handle_message(
        &mut self,
        role: RelayRole,
        relay_url: &str,
        message: RelayFrame,
    ) -> Vec<OutboundMessage> {
        match message {
            RelayFrame::Text(text) => {
                let relay = self.relay_mut(role);
                relay.counters.frames_rx = relay.counters.frames_rx.saturating_add(1);
                relay.counters.bytes_rx = relay.counters.bytes_rx.saturating_add(text.len() as u64);
                self.record_transport_rx(role, relay_url, text.len());
                let mut outbound = self.handle_text(role, relay_url, &text);
                // T117: opportunistic publish-engine retry pump. Every
                // inbound text frame ticks the engine so transient retries fire
                // as soon as their backoff is due, bounded by inbound
                // traffic frequency. The dedicated actor-tick path is a
                // follow-up (T114 is concurrently touching actor mechanics).
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

    pub(super) fn handle_text(
        &mut self,
        role: RelayRole,
        relay_url: &str,
        text: &str,
    ) -> Vec<OutboundMessage> {
        // T-relay-url-normalize: the canonical form of the delivering URL,
        // used ONLY as the `wire_subs` / `persistent_subs` map key (the EOSE
        // and CLOSED arms below). Both registration paths — `req_for_relay`
        // and the planner boundary `register_planner_wire_frames` — write
        // those maps under the canonical key, so the lookup here must
        // canonicalize to match. Without it a follow-feed sub registered with
        // a non-canonical kind:10002 URL would never satisfy
        // `is_persistent_sub` and would be wrongly auto-CLOSEd on EOSE.
        // The raw `relay_url` is deliberately left unchanged for the AUTH
        // gate / publish-engine / CLOSED classifier paths: NIP-42
        // replay-protection ties the AUTH response to the exact URL the relay
        // used, and those paths key their own per-URL state on the delivering
        // form. Falls back to wrapping the raw string for non-ws/wss inputs.
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
                    self.handle_event(role, relay_url, sub_id, event_value);
                }
            }
            "EOSE" => {
                let sub_id = array.get(1).and_then(Value::as_str).unwrap_or("unknown");
                // Extracted to `eose.rs` to keep mod.rs under the 500-LOC cap
                // (AGENTS.md hard cap). All EOSE logic lives in `handle_eose`.
                outbound.extend(self.handle_eose(role, relay_url, &wire_key_url, sub_id));
            }
            "NOTICE" => {
                let notice = array
                    .get(1)
                    .and_then(Value::as_str)
                    .map_or_else(|| "notice".to_string(), |s| truncate(s, 180));
                let relay = self.relay_mut(role);
                relay.counters.notices_rx = relay.counters.notices_rx.saturating_add(1);
                relay.last_notice = Some(notice.clone());
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
                // T133: a relay-initiated CLOSED is terminal — the relay just
                // told us the subscription is dead. Evict the row instead of
                // leaving it with `state="closed_by_relay"` (which previously
                // accumulated on the diagnostic surface across long sessions).
                // T120: the per-frame reason still flows through the classifier
                // below — the classification lands on RelayHealth.last_close_reason
                // (the diagnostic surface), so dropping the per-sub close_reason
                // here loses nothing the UI cares about.
                // #170: relay-scoped — a relay-initiated CLOSED only kills the
                // sub on the relay that sent it; a sibling relay carrying the
                // same sub_id keeps its row.
                // T-relay-url-normalize: evict by the canonical key — the row
                // was registered under the canonical URL (req_for_relay /
                // planner boundary both canonicalize).
                self.wire
                    .subs
                    .remove(&(wire_key_url.clone(), sub_id.clone()));
                if sub_id.starts_with("thread-ids-") {
                    self.thread_view.ids_inflight = false;
                }
                if sub_id.starts_with("thread-replies-") {
                    self.thread_view.replies_inflight = false;
                }
                self.changed_since_emit = true;
                // T120 (G8 / G11): apply the NIP-01 reason-prefix policy
                // table. The classifier routes by reason (auth-required
                // pauses the AuthGate; restricted/blocked mark relay
                // denied; rate-limited records for the reconnect worker;
                // error/invalid/unsupported log + give up). Pre-T120 every
                // CLOSED folded to the generic "closed_by_relay" mark.
                // T148: thread the delivering `relay_url` so the AUTH-required
                // branch can pause the right per-URL bucket in the lifecycle's
                // AuthGate, not the lane's bootstrap host.
                self.classify_and_route_closed(role, relay_url, &sub_id, reason.as_deref());
                self.sync_transport_from_lane(role, relay_url);
            }
            "OK" => {
                // M5+M2+M8 wiring: an OK frame may be the ack of an in-flight
                // kind:22242. Non-AUTH OKs are routed through the publish
                // engine (T117) — the engine's per-(event, relay) FSM folds
                // ack code + ok-bit + message into a retry verdict. Post-T105
                // the inbound `relay_url` is the resolved URL the OK arrived
                // on (per-URL transport pool), so the engine sees the same
                // URL its `dispatch` produced — not a role-bound fallback.
                // T148: thread `relay_url` so the lifecycle's per-URL AuthGate
                // un-pauses the actual socket the OK arrived on, not the lane's
                // bootstrap host.
                outbound.extend(self.handle_auth_ok(role, relay_url, array));
                outbound.extend(self.route_publish_ok(relay_url, array));
            }
            "AUTH" => {
                // M5+M2+M8 wiring: relay-initiated NIP-42 challenge. Builds the
                // kind:22242 via the bound signer (if any) and fans the new
                // RelayAuthState into the lifecycle's AuthGate so future REQs
                // to this relay are buffered until `Authenticated`. AUTH-state
                // transitions never set `changed_since_emit` — D8 invariant.
                //
                // T125: thread the DELIVERING relay's URL (not `role.url()`) so
                // the signed kind:22242 event's `["relay", ...]` tag — and the
                // outbound frame's `relay_url` routing key — both reference the
                // socket that issued the challenge. Pre-T125 both fields stamped
                // `role.bootstrap_url()`, which violated NIP-42 (replay-protection
                // semantics tie the AUTH response to the URL that sent the
                // challenge) and mis-routed the response on the URL-keyed
                // transport pool (`fada22b`).
                outbound.extend(self.handle_auth_challenge(role, relay_url, array));
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
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
