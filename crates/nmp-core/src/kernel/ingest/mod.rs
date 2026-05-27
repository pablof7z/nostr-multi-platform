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
mod profile;
mod timeline;
mod timeline_order;

use super::{
    json, truncate, CanonicalRelayUrl, Instant, Kernel, NostrEvent, OutboundMessage, RelayFrame,
    RelayRole, Value,
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
                {
                    let relay = self.relay_mut(role);
                    relay.counters.eose_rx = relay.counters.eose_rx.saturating_add(1);
                }
                self.record_transport_eose(role, relay_url);
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
                if let Some(sub) = self.wire.subs.get_mut(&wire_key) {
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
                    self.thread_view.ids_inflight = false;
                }
                if sub_id.starts_with("thread-replies-") {
                    self.thread_view.replies_inflight = false;
                }
                // T82/T104: a discovery oneshot's first stored set has landed
                // (OneShot lifecycle == "EOSE closes"). Complete + release the
                // token; the generic CLOSE below tears down the wire sub.
                // Dispatch is on the typed OneshotKind stored in oneshot_subs
                // (not a string-prefix scan — T104 typed routing).
                if self.is_discovery_oneshot(sub_id) {
                    self.complete_unknown_oneshot(sub_id);
                }
                self.record_claim_expansion_eose_no_match(sub_id, relay_url);
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
                        .wire
                        .subs
                        .get(&wire_key)
                        .map_or_else(|| relay_url.to_string(), |sub| sub.relay_url.to_string());
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
                    self.wire.subs.remove(&wire_key);
                }
                self.changed_since_emit = true;
                self.log(format!("EOSE {sub_id}"));
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
        self.record_transport_event(role, relay_url, now);
        self.events_since_last_update = self.events_since_last_update.saturating_add(1);
        self.timing.last_event_at = Some(now);
        self.timing.first_event_at.get_or_insert(now);
        // T-relay-url-normalize: the `wire_subs` row is keyed by the canonical
        // relay URL (req_for_relay / planner boundary). Canonicalize the
        // delivering URL for the lookup so the per-sub `events_rx` /
        // `last_event_at` diagnostics land on the right row regardless of the
        // delivering URL's spelling. The raw `relay_url` is preserved for
        // store provenance below.
        let wire_key_url = CanonicalRelayUrl::parse_or_raw(relay_url);
        if let Some(sub) = self.wire.subs.get_mut(&(wire_key_url, sub_id.to_string())) {
            if sub.state == "opening" {
                sub.state = "live".to_string();
            }
            sub.events_rx = sub.events_rx.saturating_add(1);
            sub.last_event_at = Some(now);
        }

        let claim_match_author = self.claim_expansion_match_author(sub_id, &event);

        // D4: all events are persisted before kind-specific dispatch.
        // Kinds 1|6 handle their own store.insert inside ingest_timeline_event.
        // For replaceable kinds (0, 3) we gate local cache mutations on the
        // store outcome: only Inserted | Replaced means this event is now
        // canonical (D4), and the same accepted event is fanned to
        // KernelEventObservers so app projections can react to kind:0/3
        // metadata without polling or app-local fetch logic. Every other
        // kind — including the former kind:10002 arm (deleted 2026-05-25
        // alongside `kernel/ingest/relay_list.rs` when the substrate parser
        // was wired in `nmp-app-template`) — routes through the wildcard arm,
        // which fans through the `EventIngestDispatcher` inside
        // `verify_and_persist` and then observes any substrate mailbox-cache
        // mutation kind-agnostically.
        match event.kind {
            1 | 6 => {
                if self.ingest_timeline_event(role, relay_url, sub_id, event) {
                    if let Some(author) = claim_match_author.as_deref() {
                        self.record_claim_expansion_hit(sub_id, relay_url, author);
                    }
                }
            }
            0 => {
                use crate::store::InsertOutcome;
                let outcome = self.verify_and_persist(relay_url, &event);
                let accepted = matches!(
                    outcome,
                    Some(InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. })
                );
                if accepted {
                    if let Some(author) = claim_match_author.as_deref() {
                        self.record_claim_expansion_hit(sub_id, relay_url, author);
                    }
                    let kernel_event = kernel_event_from_nostr(&event);
                    self.ingest_profile(event);
                    self.notify_event_observers(&kernel_event);
                }
                self.changed_since_emit = true;
            }
            3 => {
                use crate::store::InsertOutcome;
                let outcome = self.verify_and_persist(relay_url, &event);
                let accepted = matches!(
                    outcome,
                    Some(InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. })
                );
                if accepted {
                    if let Some(author) = claim_match_author.as_deref() {
                        self.record_claim_expansion_hit(sub_id, relay_url, author);
                    }
                    let kernel_event = kernel_event_from_nostr(&event);
                    self.ingest_contacts(event);
                    self.notify_event_observers(&kernel_event);
                }
                self.changed_since_emit = true;
            }
            _ => {
                // Wildcard arm: every kind not handled by an explicit match
                // arm above (NIP-65 kind:10002 mailbox lists, NIP-17
                // kind:10050 DM-relay lists, zap receipts, NIP-29 chat
                // kinds + group metadata, gift-wraps kind:1059, future
                // NIP-51 lists — all fan through the IngestParser registry
                // inside `verify_and_persist`) reaches `KernelEventObserver`s
                // through this seam. Pre-fix the wildcard called only
                // `verify_and_persist`, so projections like
                // `GroupChatProjection`, `DiscoveredGroupsProjection`, and
                // the NIP-57 zap-aggregate projection registered as
                // observers were structurally deaf. Gate fan-out on the
                // store outcome (`Inserted | Replaced` only — D4 dedup so
                // duplicate sibling-relay deliveries do not double-notify).
                //
                // V-40 — the substrate `EventIngestDispatcher` runs inside
                // `verify_and_persist` for every gated outcome, so per-NIP
                // parsers (today: `nmp_router::Kind10002Parser` and
                // `nmp_nip17::Kind10050Parser`) fire on EVERY arm (not just
                // wildcard); the kernel deliberately does not name any NIP
                // kind for dispatch purposes (D0).
                //
                // Mailbox-cache observer (replaces the deleted `10002 =>`
                // arm + `ingest::relay_list::ingest_relay_list`, 2026-05-25):
                // snapshot the substrate `MailboxCache` for `event.pubkey`
                // before dispatch, run `verify_and_persist`, then snapshot
                // again. If the cache transitioned (entry added / removed /
                // replaced) the parser populated routing state — fire the
                // `route_subscription_relays` trace observer (Debt A) and
                // enqueue the `Nip65Arrived` recompile trigger (A1) so M2
                // re-plans the author. Both calls are kind-agnostic: the
                // kernel only knows "this author's mailbox changed".
                use crate::store::InsertOutcome;
                let author = event.pubkey.clone();
                let event_id_for_trace = event.id.clone();
                let created_at_for_trigger = event.created_at;
                let before = self.mailbox_cache().snapshot(&author);
                let outcome = self.verify_and_persist(relay_url, &event);
                if matches!(
                    outcome,
                    Some(InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. })
                ) {
                    if let Some(author) = claim_match_author.as_deref() {
                        self.record_claim_expansion_hit(sub_id, relay_url, author);
                    }
                    let kernel_event = kernel_event_from_nostr(&event);
                    self.notify_event_observers(&kernel_event);
                    let after = self.mailbox_cache().snapshot(&author);
                    if before != after {
                        self.on_mailbox_changed(
                            &author,
                            &event_id_for_trace,
                            created_at_for_trigger,
                        );
                    }
                }
                self.changed_since_emit = true;
            }
        }
    }

    /// Verify and persist an event to the `EventStore`.
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
        let verified = match crate::store::VerifiedEvent::try_from_raw(raw_event_from_nostr(event))
        {
            Ok(v) => v,
            Err(e) => {
                self.log(format!(
                    "sig verify failed for {}: {e}",
                    event_short_id(&event.id)
                ));
                return None;
            }
        };
        let raw_for_observer = if self.raw_event_observers_idle_for_kind(event.kind) {
            None
        } else {
            Some(verified.raw().clone())
        };
        // V-40 — clone the verified event for the substrate
        // [`EventIngestDispatcher`] fan-out. Cloning is cheap (the inner
        // `RawEvent` is the same shape `raw_for_observer` already clones
        // above), and lets us hand `store.insert` an owned `VerifiedEvent`
        // while still feeding parsers (`Kind10050Parser`, future
        // NIP-51 parsers, …) AFTER the store gates supersession (D4).
        let verified_for_dispatch = verified.clone();
        // T105: store provenance is the *actual* URL the event came in on,
        // not the lane's bootstrap URL. The relay_count derived from store
        // sources is now correct across the URL-keyed transport pool.
        let provenance = relay_url.to_string();
        match self
            .store
            .insert(verified, &provenance, self.ingest_received_at_ms())
        {
            Ok(outcome) => {
                if raw_for_observer
                    .as_ref()
                    .is_some_and(|_| raw_tap_should_fire(&outcome))
                {
                    if let Some(raw) = raw_for_observer.as_ref() {
                        self.notify_raw_event_observers(raw, &provenance);
                    }
                }
                // V-40 — fan to substrate parsers only when the store
                // accepted this event as canonical (`Inserted | Replaced`)
                // OR when it was an ephemeral that bypassed the store. A
                // duplicate sibling-relay delivery (`Duplicate`) does NOT
                // re-fire the parser (D4 dedup).
                if matches!(
                    &outcome,
                    crate::store::InsertOutcome::Inserted { .. }
                        | crate::store::InsertOutcome::Replaced { .. }
                        | crate::store::InsertOutcome::Ephemeral { .. }
                ) {
                    // D6 — a poisoned dispatcher lock degrades to "no
                    // parser fired"; the store insert already succeeded
                    // and observers fired above, so this is the safe
                    // graceful-degrade.
                    if let Ok(d) = self.ingest_dispatcher_slot().read() {
                        d.dispatch(&verified_for_dispatch);
                    }
                }
                Some(outcome)
            }
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

    /// Substrate-honest mailbox-change observer (replaces the deleted
    /// `kernel/ingest/relay_list.rs` impl, 2026-05-25).
    ///
    /// Called from the wildcard ingest arm when the substrate
    /// [`crate::substrate::MailboxCache`] transitioned for `author`
    /// (entry added / removed / replaced by a parser the
    /// [`crate::substrate::EventIngestDispatcher`] fanned). The kernel
    /// does not know which kind triggered the mutation; it only knows the
    /// substrate cache mutated for this author.
    ///
    /// Two effects, both preserved from the pre-2026-05-25
    /// `ingest_relay_list` flow:
    ///
    /// 1. **Debt A trace fire** — call `route_subscription_relays` with the
    ///    just-updated author and the canonical content kinds (1+6) so the
    ///    injected `OutboxRouter`'s trace observer records a routing decision
    ///    attributed to lane 1 (`Nip65/Read`) reflecting the freshly-landed
    ///    state. The synthetic interest mirrors the per-author timeline
    ///    subscription shape `author_requests` builds; the returned URL set
    ///    is discarded — only the trace fire matters here.
    ///
    /// 2. **A1 recompile trigger** — enqueue
    ///    [`crate::subs::CompileTrigger::Nip65Arrived`] so the M2 subscription
    ///    compiler re-routes the author on the next `drain_tick`. The
    ///    trigger name is a historical artifact (kind:10002 is the only
    ///    kind that today writes the mailbox cache); the kernel itself
    ///    does not name the kind.
    ///
    /// 3. **Profile re-fetch** — call
    ///    [`Kernel::refresh_profile_after_mailbox`] so an already-fetched
    ///    kind:0 (necessarily fetched against the indexer lane, since
    ///    cold-start is the only state in which `pending_profile_claim_requests`
    ///    runs without a cached mailbox) is re-queued for a fresh fetch
    ///    against the author's now-known write relays. No-op when the
    ///    pubkey was never claimed.
    fn on_mailbox_changed(&mut self, author: &str, event_id: &str, created_at: u64) {
        let _ = self.route_subscription_relays(
            crate::stable_hash::stable_hash64(("mailbox-changed", event_id, created_at)),
            &[author],
            &[1, 6],
            super::mailboxes::BootstrapSeed::Discovery,
        );
        self.lifecycle
            .enqueue_trigger(crate::subs::CompileTrigger::Nip65Arrived {
                pubkey: author.to_string(),
                created_at,
            });
        self.refresh_profile_after_mailbox(author);
    }
}

fn kernel_event_from_nostr(event: &NostrEvent) -> crate::substrate::KernelEvent {
    crate::substrate::KernelEvent {
        id: event.id.clone(),
        author: event.pubkey.clone(),
        kind: event.kind,
        created_at: event.created_at,
        tags: event.tags.clone(),
        content: event.content.clone(),
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
