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
mod closed;
mod contacts;
mod profile;
mod relay_list;
mod timeline;

use super::*;

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

impl Kernel {
    pub(crate) fn handle_message(
        &mut self,
        role: RelayRole,
        relay_url: &str,
        message: Message,
    ) -> Vec<OutboundMessage> {
        match message {
            Message::Text(text) => {
                let relay = self.relay_mut(role);
                relay.counters.frames_rx = relay.counters.frames_rx.saturating_add(1);
                relay.counters.bytes_rx = relay.counters.bytes_rx.saturating_add(text.len() as u64);
                let mut outbound = self.handle_text(role, relay_url, &text);
                // T117: opportunistic publish-engine retry pump. Every
                // inbound text frame ticks the engine so transient retries fire
                // as soon as their backoff is due, bounded by inbound
                // traffic frequency. The dedicated actor-tick path is a
                // follow-up (T114 is concurrently touching actor mechanics).
                outbound.extend(self.tick_publish_engine_for_now());
                outbound
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
                {
                    let relay = self.relay_mut(role);
                    relay.counters.eose_rx = relay.counters.eose_rx.saturating_add(1);
                }
                // T105: the follow-feed (seed-timeline) is now per-relay
                // (`seed-timeline-<short-hash>`). Both the legacy id and its
                // per-relay variants stay live after EOSE. Persistent subs
                // (NWC kind:23195 listener, …) registered via
                // `register_persistent_sub` also survive EOSE.
                let keep_live = sub_id == "seed-timeline"
                    || sub_id.starts_with("seed-timeline-")
                    || sub_id.starts_with("diag-firehose-")
                    || self.is_persistent_sub(&wire_key_url, sub_id);
                let wire_key = (wire_key_url.clone(), sub_id.to_string());
                if let Some(sub) = self.wire_subs.get_mut(&wire_key) {
                    sub.eose_at = Some(Instant::now());
                    if keep_live {
                        sub.state = "live".to_string();
                    } else {
                        // T133: mark closed for the brief window before
                        // eviction below; ingest path readers (e.g. EVENT for
                        // an already-EOSE'd sub) will see the row absent.
                        sub.state = "closed".to_string();
                    }
                }
                if sub_id.starts_with("thread-ids-") {
                    self.thread_ids_inflight = false;
                }
                if sub_id.starts_with("thread-replies-") {
                    self.thread_replies_inflight = false;
                }
                // T82/T104: a discovery oneshot's first stored set has landed
                // (OneShot lifecycle == "EOSE closes"). Complete + release the
                // token; the generic CLOSE below tears down the wire sub.
                // Dispatch is on the typed OneshotKind stored in oneshot_subs
                // (not a string-prefix scan — T104 typed routing).
                if self.is_discovery_oneshot(sub_id) {
                    self.complete_unknown_oneshot(sub_id);
                }
                if !keep_live {
                    // T105: CLOSE must travel back to the same socket the REQ
                    // went out on — the transport pool is URL-keyed, so a
                    // role-only close would target the bootstrap socket and
                    // leave the resolved sub open. Pull the recorded URL from
                    // the WireSub set on req_for_relay; fall back to the
                    // delivering relay's URL when the sub_id is unknown.
                    // #170: the CLOSE travels back on the SAME socket the
                    // EOSE arrived on (relay_url) — the wire_subs key is now
                    // relay-scoped so the row, if any, is this relay's row,
                    // not a sibling's. Fall back to the delivering URL.
                    let close_url = self
                        .wire_subs
                        .get(&wire_key)
                        .map(|sub| sub.relay_url.to_string())
                        .unwrap_or_else(|| relay_url.to_string());
                    outbound.push(OutboundMessage {
                        role,
                        relay_url: close_url,
                        text: json!(["CLOSE", sub_id]).to_string(),
                    });
                    // T133: evict the row now that the CLOSE outbound is
                    // queued. The closed state is logically terminal for any
                    // sub that is not the live follow-feed / firehose; keeping
                    // the row was a diagnostic-only courtesy that grew the
                    // table unboundedly across long sessions (every
                    // profile-claim, thread-ids, thread-replies, and discovery
                    // oneshot completes via this EOSE→CLOSE path).
                    self.wire_subs.remove(&wire_key);
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
                self.wire_subs
                    .remove(&(wire_key_url.clone(), sub_id.clone()));
                if sub_id.starts_with("thread-ids-") {
                    self.thread_ids_inflight = false;
                }
                if sub_id.starts_with("thread-replies-") {
                    self.thread_replies_inflight = false;
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

    pub(super) fn handle_event(
        &mut self,
        role: RelayRole,
        relay_url: &str,
        sub_id: &str,
        value: &Value,
    ) {
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
        // T-relay-url-normalize: the `wire_subs` row is keyed by the canonical
        // relay URL (req_for_relay / planner boundary). Canonicalize the
        // delivering URL for the lookup so the per-sub `events_rx` /
        // `last_event_at` diagnostics land on the right row regardless of the
        // delivering URL's spelling. The raw `relay_url` is preserved for
        // store provenance below.
        let wire_key_url = CanonicalRelayUrl::parse_or_raw(relay_url);
        if let Some(sub) = self
            .wire_subs
            .get_mut(&(wire_key_url, sub_id.to_string()))
        {
            if sub.state == "opening" {
                sub.state = "live".to_string();
            }
            sub.events_rx = sub.events_rx.saturating_add(1);
            sub.last_event_at = Some(now);
        }

        // Raw signed-event tap (additive, generic — D0). This is the SINGLE
        // all-kinds ingest point: every kind flows through here with the
        // full `NostrEvent` (`event.sig` intact) before the per-kind
        // dispatch below projects it to the sig-stripped `KernelEvent`.
        // Some consumers need the verbatim signed event (`sig` included) —
        // e.g. an inbound-ingest seam that must hand the whole signed event
        // to its own state machine. Mutating `KernelEvent` to carry `sig`
        // would couple every projection consumer to that need; this
        // parallel tap keeps the projection type stable.
        //
        // Cost discipline (D8): the idle fast-path probe short-circuits
        // BEFORE any allocation when no registration filters on this kind
        // (the common case — nobody tapped). Only when a consumer IS
        // listening for `event.kind` do we pay to build a `RawEvent` and
        // run `VerifiedEvent::try_from_raw` (the kernel's existing Schnorr
        // + id-hash gate). The tap fires ONLY on gate success, so a
        // consumer never sees an unverified event. The per-kind dispatch
        // below independently re-verifies through its own store path; the
        // duplicate verify is the deliberate, documented cost of keeping
        // this a fully additive tap that does not touch projection / subs
        // / per-kind handlers (a single-verify refactor is future work).
        if !self.raw_event_observers_idle_for_kind(event.kind) {
            let raw = raw_event_from_nostr(&event);
            match crate::store::VerifiedEvent::try_from_raw(raw) {
                Ok(verified) => self.notify_raw_event_observers(verified.raw()),
                Err(e) => self.log(format!(
                    "raw-tap sig verify failed for {}: {e}",
                    event_short_id(&event.id)
                )),
            }
        }

        // D4: all events are persisted before kind-specific dispatch.
        // Kinds 1|6 handle their own store.insert inside ingest_timeline_event.
        // For replaceable kinds (0, 3, 10002) we gate local cache mutations on
        // the store outcome: only Inserted | Replaced means this event is now
        // canonical (D4).
        match event.kind {
            1 | 6 => self.ingest_timeline_event(role, relay_url, sub_id, event),
            0 => {
                use crate::store::InsertOutcome;
                let outcome = self.verify_and_persist(relay_url, &event);
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
                let outcome = self.verify_and_persist(relay_url, &event);
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
                let outcome = self.verify_and_persist(relay_url, &event);
                if matches!(
                    outcome,
                    Some(InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. })
                ) {
                    self.ingest_relay_list(event);
                }
                self.changed_since_emit = true;
            }
            _ => {
                self.verify_and_persist(relay_url, &event);
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
        relay_url: &str,
        event: &NostrEvent,
    ) -> Option<crate::store::InsertOutcome> {
        let verified = match crate::store::VerifiedEvent::try_from_raw(raw_event_from_nostr(event)) {
            Ok(v) => v,
            Err(e) => {
                self.log(format!(
                    "sig verify failed for {}: {e}",
                    event_short_id(&event.id)
                ));
                return None;
            }
        };
        // T105: store provenance is the *actual* URL the event came in on,
        // not the lane's bootstrap URL. The relay_count derived from store
        // sources is now correct across the URL-keyed transport pool.
        let provenance = relay_url.to_string();
        match self
            .store
            .insert(verified, &provenance, self.ingest_received_at_ms())
        {
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

    /// Wall-clock arrival timestamp (unix millis) for a store insert.
    ///
    /// Clock seam (kernel/clock.rs): `received_at_ms` is reducer output —
    /// it is written into the `EventStore` — so it MUST read the injected
    /// `Clock` rather than `SystemTime::now()` directly, otherwise
    /// deterministic replay diverges (D9: the kernel owns time).
    pub(in crate::kernel) fn ingest_received_at_ms(&self) -> u64 {
        self.clock
            .now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}
