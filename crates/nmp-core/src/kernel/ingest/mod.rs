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
mod claimed_event_stamp; // ADR-0055 Rung 1 (F1) claimed-event stamp — sibling for size baseline
mod closed;
mod contacts;
// EOSE frame handling (incl. K3 Stage D1 coverage write), split for the LOC cap.
mod eose;
// `pub(in crate::kernel)`: shares `kernel_event_from_nostr` with the
// local-publish-intent path (read-your-writes fan-out, one construction site).
pub(in crate::kernel) mod helpers;
mod profile;
mod timeline;
mod timeline_order;
use super::{
    json, truncate, CanonicalRelayUrl, Instant, Kernel, NostrEvent, OutboundMessage, RelayFrame,
    RelayRole, Value,
};

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
                // Full EOSE handling (keep-live decision, F-TTL freshness stamp,
                // K3 Stage D1 coverage write, CLOSE/evict) lives in `eose.rs`.
                self.handle_eose(role, relay_url, sub_id, &wire_key_url, &mut outbound);
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
                // V-112 (ADR-0042): thread-ids-/thread-replies- inflight flags deleted.
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
        // V-68 / V-112 (ADR-0042): maybe_open_thread_hydration() deleted.
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
        // was wired in `nmp-defaults`) — routes through the wildcard arm,
        // which fans through the `EventIngestDispatcher` inside
        // `verify_and_persist` and then observes any substrate mailbox-cache
        // mutation kind-agnostically.
        match event.kind {
            1 | 6 => {
                // W8b: capture event_id before ingest_timeline_event consumes `event`.
                let event_id = event.id.clone();
                if self.ingest_timeline_event(role, relay_url, sub_id, event) {
                    if let Some(author) = claim_match_author.as_deref() {
                        self.record_claim_expansion_hit(sub_id, relay_url, author, &event_id);
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
                        self.record_claim_expansion_hit(sub_id, relay_url, author, &event.id);
                    }
                    let kernel_event = helpers::kernel_event_from_nostr(&event);
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
                        self.record_claim_expansion_hit(sub_id, relay_url, author, &event.id);
                    }
                    let kernel_event = helpers::kernel_event_from_nostr(&event);
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
                // through this seam (e.g. `GroupChatProjection`,
                // `DiscoveredGroupsProjection`, the NIP-57 zap-aggregate
                // projection). Gate fan-out on the store outcome
                // (`Inserted | Replaced` only — D4 dedup so duplicate
                // sibling-relay deliveries do not double-notify).
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
                //
                // F-02 — symmetric snapshot for the substrate
                // `DmInboxRelayLookup` (NIP-17 kind:10050 DM-relay list).
                // The `Kind10050Parser` writes this cache inside
                // `verify_and_persist`; without detecting the transition the
                // kernel would never enqueue the `DmRelayListChanged` recompile
                // trigger, so a cold-start gift-wrap inbox interest
                // (kind:1059 `#p`, `PTagRouting::Nip17DmRelays`) pushed before
                // the kind:10050 round-trip closed would stay compiled to
                // "no subscription" forever (fail-closed routing in
                // `active_giftwrap_inbox_interest`). Kind-agnostic: the kernel
                // only knows "this author's DM-relay list may have changed".
                use crate::store::InsertOutcome;
                let author = event.pubkey.clone();
                let event_id_for_trace = event.id.clone();
                let created_at_for_trigger = event.created_at;
                let before = self.mailbox_cache().snapshot(&author);
                let dm_before = self.recipient_dm_relays(&author);
                let outcome = self.verify_and_persist(relay_url, &event);
                if matches!(
                    outcome,
                    Some(InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. })
                ) {
                    if let Some(author) = claim_match_author.as_deref() {
                        // W8b: `event_id_for_trace` holds the event id captured
                        // before `verify_and_persist` consumed the event borrow.
                        self.record_claim_expansion_hit(
                            sub_id,
                            relay_url,
                            author,
                            &event_id_for_trace,
                        );
                    }
                    let kernel_event = helpers::kernel_event_from_nostr(&event);
                    self.notify_event_observers(&kernel_event);
                    let after = self.mailbox_cache().snapshot(&author);
                    if before != after {
                        self.on_mailbox_changed(
                            &author,
                            &event_id_for_trace,
                            created_at_for_trigger,
                        );
                    }
                    // F-02 — DM-relay-list transition: enqueue the
                    // `DmRelayListChanged` recompile trigger so the planner
                    // re-routes `PTagRouting::Nip17DmRelays` interests against
                    // the freshly-populated cache on the next
                    // `drain_lifecycle_tick`.
                    let dm_after = self.recipient_dm_relays(&author);
                    if dm_before != dm_after {
                        self.on_dm_relays_changed(&author, created_at_for_trigger);
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
        let verified = match crate::store::VerifiedEvent::try_from_raw(helpers::raw_event_from_nostr(event))
        {
            Ok(v) => v,
            Err(e) => {
                self.log(format!(
                    "sig verify failed for {}: {e}",
                    helpers::event_short_id(&event.id)
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
                    .is_some_and(|_| helpers::raw_tap_should_fire(&outcome))
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

                // F-TTL — replaceable/addressable event freshness hook.
                //
                // When a canonical (regular) replaceable or addressable event is
                // ingested, stamp its `check_again_after` so the kernel's TTL gate
                // (claim_replaceable) knows it is fresh and does not immediately
                // re-REQ it. Addressable events are keyed by their `d`-tag.
                //
                // D9 clock seam: `now_ms()` reads the injected `Clock`, never
                // `SystemTime::now()` directly — so this is deterministic under
                // replay/FixedClock.
                let is_regular = crate::store::is_replaceable(event.kind);
                let is_addressable = crate::store::is_parameterized_replaceable(event.kind);
                if is_regular || is_addressable {
                    if let Some(pubkey_bytes) =
                        crate::kernel::hex_to_pubkey_bytes(&event.pubkey)
                    {
                        let key = if is_addressable {
                            let d_tag = event
                                .tags
                                .iter()
                                .find(|t| t.first().map(|s| s == "d").unwrap_or(false))
                                .and_then(|t| t.get(1))
                                .cloned()
                                .unwrap_or_default();
                            crate::store::ReplaceableKey::Parameterized {
                                kind: event.kind,
                                pubkey: pubkey_bytes,
                                d_tag,
                            }
                        } else {
                            crate::store::ReplaceableKey::Regular {
                                kind: event.kind,
                                pubkey: pubkey_bytes,
                            }
                        };
                        let ttl_ms =
                            self.replaceable_ttl.ttl_for_kind(event.kind).as_millis() as u64;
                        self.store
                            .set_check_again_after(key, self.now_ms() + ttl_ms);
                    }
                }

                self.maybe_bump_claimed_event_content(&outcome, &event); // ADR-0055 (F1)
                Some(outcome)
            }
            Err(e) => {
                self.log(format!(
                    "store insert error for {}: {e}",
                    helpers::event_short_id(&event.id)
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
            .duration_since(super::UNIX_EPOCH)
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
    ///    just-updated author and an EMPTY kind slice so the injected
    ///    `OutboxRouter`'s trace observer records a routing decision
    ///    attributed to lane 1 (`Nip65/Read`) reflecting the freshly-landed
    ///    state. The returned URL set is discarded — only the trace fire
    ///    matters here.
    ///
    ///    V-68 / D0: the kind set is NOT a substrate default. This is a
    ///    mailbox-change observer that fires for *any* author whose NIP-65
    ///    relay list mutated — it has no app-timeline concept to declare, and
    ///    is not coupled to the follow-feed's host-declared
    ///    `follow_feed_kinds`. The read-lane routing decision is independent of
    ///    `kinds` (`is_discovery_kind` covers only {0, 3, 10000–19999}; content
    ///    kinds like 1/6 never alter the lane), so passing `&[]` is the honest,
    ///    policy-free choice — it removes the prior hardcoded `{1, 6}` social
    ///    default without changing routing behavior.
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
            &[], // V-68/D0: no substrate social default; trace lane is kind-independent.
            super::mailboxes::BootstrapSeed::Discovery,
        );
        self.lifecycle
            .enqueue_trigger(crate::subs::CompileTrigger::Nip65Arrived {
                pubkey: author.to_string(),
                created_at,
            });
        self.refresh_profile_after_mailbox(author);
    }

    /// F-02 — substrate-honest DM-relay-list-change observer.
    ///
    /// Called from the wildcard ingest arm when the substrate
    /// [`crate::substrate::DmInboxRelayLookup`] transitioned for `author`
    /// (a NIP-17 kind:10050 was added / removed / replaced by the
    /// `Kind10050Parser` the [`crate::substrate::EventIngestDispatcher`]
    /// fanned). The kernel does not name the kind — it only observes that
    /// the substrate DM-relay cache mutated for this author.
    ///
    /// Enqueues [`crate::subs::CompileTrigger::DmRelayListChanged`] so the
    /// planner re-routes every interest whose `#p` routing mode is
    /// [`crate::planner::PTagRouting::Nip17DmRelays`] (today: the
    /// gift-wrap inbox interest from `nmp_nip17::active_giftwrap_inbox_interest`)
    /// against the freshly-populated cache on the next `drain_lifecycle_tick`.
    ///
    /// This is the production seam the V-40 migration left as a follow-up
    /// (see `kernel/test_support.rs::seed_kind10050_for_test`, which drives
    /// the equivalent trigger inline for `nmp-core`-internal tests). Its
    /// absence was the F-02 cold-start defect: a returning user with a
    /// kind:10050 on a prior device fetched that list on sign-in, but the
    /// gift-wrap inbox interest — pushed by the host DM runtime before the
    /// fetch closed — never recompiled, so the kind:1059 `#p` REQ never went
    /// out and the DM inbox stayed empty.
    pub(super) fn on_dm_relays_changed(&mut self, author: &str, created_at: u64) {
        self.lifecycle
            .enqueue_trigger(crate::subs::CompileTrigger::DmRelayListChanged {
                pubkey: author.to_string(),
                created_at,
            });
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
