//! Relay-frame parsing and the single accepted-event ingest chokepoint.
//!
//! ADR-0057 — `handle_message` → `handle_text` → `handle_event` does the
//! relay-only bookkeeping (relay counters, transport provenance, wire-sub
//! diagnostics, claim-expansion match) then hands the parsed event to the ONE
//! kind-agnostic, source-agnostic chokepoint [`Kernel::ingest_accepted_event`].
//! The chokepoint replaces the two hand-maintained per-kind ingest ladders
//! (the old relay `match event.kind` arms here and the deleted
//! `record_local_publish_intent` mirror in `local_publish_intent.rs`).
//!
//! The chokepoint separates three concerns into three layers:
//! - **Admission** = valid signature only (inside [`Kernel::verify_and_persist`]).
//! - **Delivery vs persistence** = gated by the store [`crate::store::InsertOutcome`].
//!   `verify_and_persist` does PERSISTENCE ONLY (sig-verify → `store.insert` →
//!   raw-tap → provenance → TTL) and returns the `(InsertOutcome, VerifiedEvent)`.
//!   The shared [`Kernel::project_accepted_event`] then fires BOTH the NIP-parser
//!   [`crate::substrate::EventIngestDispatcher`] dispatch AND the app-facing
//!   `KernelEventObserver` notify on the canonical accepted outcome
//!   (`Inserted | Replaced | Ephemeral`) — so an ephemeral reaches both the
//!   parsers and the app observers (ADR-0057 §1 latent-bug fix), and a
//!   `Duplicate` (incl. the relay echo of a local publish) is projection-silent
//!   (D4 single-fire / read-your-writes). `project_accepted_event` is the ONE
//!   post-store fan-out, called by both the live chokepoint
//!   ([`Kernel::ingest_accepted_event`]) and cache-serve replay
//!   ([`Kernel::feed_served_event`]), so the two paths cannot diverge.
//! - **Projection / relevance** = read-time only. The kernel-owned post-store
//!   read-cache (the timeline read-cache projection) is CALLED BY the chokepoint,
//!   gated by the behavioral `follow_feed_kinds` predicate (D0, no kind literal).
//!   Profiles (kind:0, ADR-0057 PR 2) AND contacts (kind:3, ADR-0057 PR 3) moved
//!   out to registered `nmp_nip01::Kind0Parser` / `Kind3Parser` writing the
//!   capability-owned `ProfileCache` / `ContactsCache` — both detected via a
//!   before/after cache snapshot exactly like the mailbox / DM-relay observers.
//!   For contacts the kernel additionally reacts to the ACTIVE account's
//!   transition by driving the kernel-owned follow-feed effects
//!   (`on_active_contacts_changed`: `timeline_authors` rebuild,
//!   `sync_follow_feed_interests`, `FollowListChanged`, cache-serve) — the
//!   PARSER stays side-effect-free against kernel state; the KERNEL owns the
//!   effects, driven by the transition SIGNAL (never inlined in the parser).
//!   Substrate `MailboxCache` / `DmInboxRelayLookup` transitions are likewise
//!   detected kind-agnostically by bracketing the chokepoint with before/after
//!   snapshots (the kernel only knows "this author's mailbox / contacts
//!   changed", never "a kind:10002 / kind:3 arrived" —
//!   `docs/architecture/crate-boundaries.md` §0).
//!
//! Local publishes enter the chokepoint at `publish_engine.rs` with
//! `local://publish` provenance ([`IngestSource::LocalPublish`]); cache-replay
//! keeps its ADR-0045 path (`cache_serve/continuation.rs::feed_served_event`).
//!
//! ADR-0057 PR 3 is the full D0 finish-line: the kernel ingest path now names
//! ZERO NIP kind literals. kind:0 (profiles) moved in PR 2, kind:3 (contacts)
//! moves here; the 1/6 timeline gate is the behavioral `follow_feed_kinds`
//! predicate, and kind:1059 gift-wrap stays excluded via the parser registry,
//! not a literal.

mod auth_handlers;
mod claimed_event_stamp; // ADR-0055 Rung 1 (F1) claimed-event stamp — sibling for size baseline
mod closed;
mod contacts;
// EOSE frame handling (incl. K3 Stage D1 coverage write), split for the LOC cap.
mod eose;
// `pub(in crate::kernel)`: shares `kernel_event_from_nostr` with the
// local-publish-intent path (read-your-writes fan-out, one construction site).
pub(in crate::kernel) mod helpers;
mod timeline;
mod timeline_order;
use super::{
    truncate, CanonicalRelayUrl, Instant, Kernel, NostrEvent, OutboundMessage, RelayFrame,
    RelayRole, Value,
};

/// ADR-0057 — provenance discriminator for the single accepted-event
/// chokepoint ([`Kernel::ingest_accepted_event`]).
///
/// The chokepoint is source-agnostic for persistence + delivery, but each
/// source carries a distinct provenance encoding that ADR-0057 preserves
/// verbatim (it does NOT introduce the typed `Provenance` enum — that is left
/// to the ADR-0045 amendment that names `Provenance::LocalStore`). `Relay`
/// additionally carries the wire `sub_id` so the timeline projection's
/// read-time relevance predicate can consult oneshot / firehose / open-interest
/// sub schemes. Cache-replay keeps its own ADR-0045 path
/// (`feed_served_event`) and does not flow through this enum.
pub(in crate::kernel) enum IngestSource<'a> {
    /// A relay-delivered event. Provenance = the delivering relay URL; the
    /// wire `sub_id` feeds the timeline read-time relevance predicate.
    Relay { relay_url: &'a str, sub_id: &'a str },
    /// A locally-published event accepted by the publish engine. Provenance =
    /// the literal `local://publish`; there is no wire sub.
    LocalPublish,
}

impl IngestSource<'_> {
    /// The store-insert provenance string for this source.
    fn provenance(&self) -> &str {
        match self {
            IngestSource::Relay { relay_url, .. } => relay_url,
            IngestSource::LocalPublish => "local://publish",
        }
    }

    /// The wire `sub_id` for relay deliveries; empty for local publishes
    /// (a local publish has no wire sub, and an empty id cannot collide with
    /// the prefix-matched sub schemes consulted by `should_store_event`).
    fn sub_id(&self) -> &str {
        match self {
            IngestSource::Relay { sub_id, .. } => sub_id,
            IngestSource::LocalPublish => "",
        }
    }
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
        // ADR-0057 — relay-only bookkeeping above this line stays in
        // `handle_event` (relay counters, transport provenance, wire-sub
        // diagnostics). The claim-expansion *match* is computed here (it needs
        // the wire `sub_id`), but the claim-hit *scoring* is a relay-only
        // wrapper applied AFTER the shared chokepoint returns — see below.
        let claim_match_author = self.claim_expansion_match_author(sub_id, &event);
        // Captured before the chokepoint consumes `event` (claim-hit scoring
        // below needs the id).
        let event_id_for_score = event.id.clone();

        // The shared, kind-agnostic, source-agnostic accepted-event chokepoint
        // (ADR-0057). It owns sig-verify → `store.insert` → NIP-parser dispatch
        // → app-observer notify → the kernel-owned post-store cache routing
        // (profile / contacts / timeline projection / mailbox + DM-relay
        // transition observers). The relay path passes the delivering URL as
        // provenance; `IngestSource::Relay` carries the wire `sub_id` so the
        // timeline projection's read-time relevance predicate
        // (`should_store_event`) can consult oneshot / firehose / open-interest
        // sub schemes.
        let outcome = self.ingest_accepted_event(IngestSource::Relay { relay_url, sub_id }, event);

        // Relay claim-hit scoring stays a relay-only wrapper after the helper
        // returns (ADR-0057): a canonical accept (`Inserted | Replaced`) on a
        // sub that matched a claim-expansion shape records the hit.
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
    }

    /// ADR-0057 — the single kind-agnostic, source-agnostic accepted-event
    /// chokepoint. Replaces the two hand-maintained per-kind ingest ladders
    /// (`handle_event`'s relay `match event.kind` arms and the deleted
    /// `record_local_publish_intent` mirror arms).
    ///
    /// Three concerns are now three layers:
    ///
    /// 1. **Admission** = valid signature only — enforced inside
    ///    [`Self::verify_and_persist`] (`try_from_raw`). No relevance gate, no
    ///    acquisition-match, no kind gate.
    /// 2. **Delivery vs persistence** = gated by the store [`InsertOutcome`].
    ///    `verify_and_persist` does persistence ONLY (`Inserted | Replaced`;
    ///    ephemerals return `Ephemeral` un-stored) and returns the
    ///    `(InsertOutcome, VerifiedEvent)`. The shared
    ///    [`Self::project_accepted_event`] then fires the NIP-parser dispatch +
    ///    the app-facing [`Kernel::notify_event_observers`] seam on the canonical
    ///    accepted outcome (`Inserted | Replaced | Ephemeral`). `Duplicate` (incl. a
    ///    relay echo of a locally-published event) is projection-silent —
    ///    preserving D4 single-fire for read-your-writes.
    /// 3. **Projection / relevance** = read-time only. The kernel-owned timeline
    ///    read-cache is CALLED BY this one chokepoint post-`verify_and_persist`,
    ///    gated on the behavioral `follow_feed_kinds` predicate — not a scattered
    ///    per-kind/per-source ladder. The timeline read-cache is a chokepoint-fed
    ///    observer ([`Self::project_timeline_event`]) whose read-time relevance
    ///    predicate is `should_store_event` (which no longer has any power over
    ///    persistence).
    ///
    /// ADR-0057 PR 3 finishes D0: profiles (kind:0) AND contacts (kind:3) are
    /// both parser-fed (`nmp_nip01::Kind0Parser` / `Kind3Parser` writing the
    /// capability-owned `ProfileCache` / `ContactsCache`), detected via a
    /// before/after cache snapshot in [`Self::project_accepted_event`]. The
    /// ingest path now names ZERO NIP kind literals — the timeline projection is
    /// gated by the behavioral `follow_feed_kinds` predicate, and gift-wrap is
    /// excluded via the parser registry, not a literal.
    ///
    /// Returns the store outcome so a source wrapper can apply source-specific
    /// post-processing (e.g. the relay path's claim-hit scoring).
    pub(in crate::kernel) fn ingest_accepted_event(
        &mut self,
        source: IngestSource<'_>,
        event: NostrEvent,
    ) -> Option<crate::store::InsertOutcome> {
        use crate::store::InsertOutcome;

        let provenance = source.provenance();
        let sub_id = source.sub_id();

        // Persistence ONLY: sig-verify → store.insert → raw-tap → provenance
        // accounting → TTL stamping (ADR-0057). Returns the verified clone so
        // the shared projection helper runs without re-verifying the signature.
        let Some((outcome, verified)) = self.verify_and_persist(provenance, &event) else {
            // Sig-verify (or store insert) failed — nothing to project.
            return None;
        };

        // The single shared post-store projection fan-out (parser dispatch +
        // capability-cache transition sweep + D9-clamped app-observer notify),
        // gated on the canonical accepted outcome `Inserted | Replaced |
        // Ephemeral` — the SAME helper the cache-serve replay path runs, so the
        // two cannot diverge. A `Duplicate` (incl. the relay echo of a local
        // publish) is projection-silent (D4 single-fire).
        let canonical = matches!(
            outcome,
            InsertOutcome::Inserted { .. }
                | InsertOutcome::Replaced { .. }
                | InsertOutcome::Ephemeral { .. }
        );
        if canonical {
            self.project_accepted_event(&verified);
        }

        // Timeline read-cache projection — LIVE-path specific (the cache-serve
        // path has its own follow-set timeline append in `feed_served_event`).
        // Gated by the behavioral `follow_feed_kinds` predicate (D0, no kind
        // literal); the `diag-firehose-` clause recognizes the kernel's
        // content-firehose diagnostic sub scheme so a firehose stress sub
        // projects regardless of the host-declared follow-feed kinds. The
        // `Duplicate` `relay_count` bump (a diagnostic signal, NOT a projection
        // mutation) is preserved inside `project_timeline_event` even though a
        // `Duplicate` is projection-silent for observers.
        if self.follow_feed_kinds.contains(&event.kind) || sub_id.starts_with("diag-firehose-") {
            self.project_timeline_event(sub_id, &event, Some(&outcome));
        }

        // ADR-0057 PR 3 — the kind:3 arm is DELETED. Contacts are now
        // parser-fed: `nmp_nip01::Kind3Parser` writes the capability-owned
        // `ContactsCache` from the `EventIngestDispatcher` fan-out inside
        // `project_accepted_event` above, and the active-account contacts
        // transition detected there drives the kernel-owned follow-feed effects
        // (`on_active_contacts_changed`). The ingest path no longer names
        // kind:3 (or any NIP kind literal — D0 finish-line). kind:0 went in
        // PR 2; the timeline 1/6 gate is the behavioral `follow_feed_kinds`
        // predicate, not a literal.

        Some(outcome)
    }

    /// Verify and persist an event to the `EventStore` — persistence ONLY
    /// (ADR-0057: sig-verify → store.insert → raw-tap → provenance accounting →
    /// TTL stamping). The post-store projection fan-out (parser dispatch +
    /// transition sweep + clamped observer notify) is the caller's job via the
    /// shared [`Self::project_accepted_event`] helper.
    ///
    /// Returns `Some((outcome, verified))` when sig-verify succeeds — the
    /// `verified` clone lets the caller run projection without re-verifying —
    /// or `None` when signature verification (or the store insert) fails.
    /// Callers that perform local-cache mutations for replaceable kinds **must**
    /// inspect the outcome: only `Inserted | Replaced` means this event is now
    /// the canonical version in the store — all other outcomes must be treated
    /// as no-ops for cache purposes (D4).
    pub(super) fn verify_and_persist(
        &mut self,
        relay_url: &str,
        event: &NostrEvent,
    ) -> Option<(crate::store::InsertOutcome, crate::store::VerifiedEvent)> {
        let verified =
            match crate::store::VerifiedEvent::try_from_raw(helpers::raw_event_from_nostr(event)) {
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
                // T131 — per-URL `RelayUsefulness` provenance accounting.
                // ADR-0057: this moved out of `ingest_timeline_event` (which is
                // deleted) into the chokepoint, so it now runs uniformly for
                // EVERY event/kind on the single ingest path, not just
                // kind:1/6 timeline events. The outcome → counter mapping is
                // unchanged: a novel `Inserted` credits the first source URL, a
                // `Replaced` / `Duplicate` / `Rejected` records the redundant
                // or rejected delivery; protocol-state transitions
                // (`Superseded` / `Tombstoned` / `Ephemeral`) are not
                // relay-usefulness signals.
                match &outcome {
                    crate::store::InsertOutcome::Inserted { .. } => {
                        self.event_provenance
                            .record_first_source(&event.id, &provenance);
                    }
                    crate::store::InsertOutcome::Replaced { .. } => {
                        self.event_provenance.record_replaced(&provenance);
                    }
                    crate::store::InsertOutcome::Duplicate { .. } => {
                        self.event_provenance.record_duplicate(&provenance);
                    }
                    crate::store::InsertOutcome::Rejected { .. } => {
                        self.event_provenance.record_rejected(&provenance);
                    }
                    crate::store::InsertOutcome::Superseded { .. }
                    | crate::store::InsertOutcome::Tombstoned { .. }
                    | crate::store::InsertOutcome::Ephemeral { .. } => {}
                }
                // ADR-0057 — the post-store projection fan-out (NIP-parser
                // dispatch + transition sweep + D9-clamped app-observer notify)
                // moved OUT of `verify_and_persist` into the single shared
                // [`Self::project_accepted_event`] helper, called by BOTH this
                // live chokepoint (`ingest_accepted_event`) AND the cache-serve
                // replay path (`feed_served_event`) so the two cannot diverge.
                // `verify_and_persist` now owns ONLY persistence (sig-verify →
                // store.insert → raw-tap → provenance accounting → TTL stamping);
                // the caller runs `project_accepted_event` on the accepted gate
                // (`Inserted | Replaced | Ephemeral`). The clone built for the
                // dispatcher is returned to the caller so projection runs without
                // re-verifying the signature.

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
                    if let Some(pubkey_bytes) = crate::kernel::hex_to_pubkey_bytes(&event.pubkey) {
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
                Some((outcome, verified_for_dispatch))
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

    /// ADR-0057 — the SINGLE shared post-store projection fan-out, called by
    /// BOTH the live ingest chokepoint ([`Self::ingest_accepted_event`]) AND the
    /// cache-serve replay path ([`Self::feed_served_event`]). Unifying these two
    /// sites means they cannot diverge: a fix here (the D9 clamp, a new
    /// capability-cache transition) applies to BOTH live ingest and cold-restart
    /// cache-serve at once — no per-kind / per-cache re-patching.
    ///
    /// It owns the three post-store concerns, kind-agnostically:
    ///
    /// 1. **NIP-parser dispatch** — fan `verified` to every registered
    ///    [`crate::substrate::IngestParser`]. These write the capability-owned
    ///    caches (profile kind:0, mailbox kind:10002, DM-relay kind:10050, …)
    ///    BETWEEN the before/after transition snapshots below.
    /// 2. **Capability-cache transition sweep** — snapshot mailbox / DM-relay /
    ///    profile for the author BEFORE dispatch, compare AFTER, and on a real
    ///    transition fire the kernel-owned effect: `on_mailbox_changed`
    ///    (`Nip65Arrived` recompile + routing trace), `on_dm_relays_changed`
    ///    (`DmRelayListChanged` recompile), and the profile rev bump
    ///    (`profiles_ver` + `claimed_event_content_ver` when `event_claims` is
    ///    non-empty, plus byte-estimate invalidation). The kernel never names a
    ///    NIP kind — it only knows "this author's cached X may have changed".
    /// 3. **App-observer notify (D9-clamped)** — deliver a [`KernelEvent`] to the
    ///    app-facing `KernelEventObserver` seam with a future-dated `created_at`
    ///    clamped to `now`. The observer fan-out is the input to every app feed
    ///    (`nmp-feed` / `nmp-nip01::FlatFeed` order by `created_at`), so a
    ///    hostile/buggy relay's future-dated event would otherwise pin to the TOP
    ///    of every consumer's feed — and a stored future-dated event would defeat
    ///    the live clamp by surviving a restart and being cache-served. The
    ///    authoritative store row and the in-memory read-cache `StoredEvent`
    ///    retain the raw wire timestamp for protocol correctness; only the
    ///    observer-delivered shape is clamped (same kernel-owned `now_secs()`
    ///    clock — D9, one time source).
    ///
    /// **ADR-0045 invariant:** this helper NEVER calls `store.insert`. The live
    /// path persists first (`verify_and_persist`) then projects; the cache-serve
    /// path reads an already-stored event and only projects. Both run exactly
    /// this fan-out.
    ///
    /// Callers MUST gate on the canonical accepted outcome
    /// (`Inserted | Replaced | Ephemeral`) — a `Duplicate` (incl. the relay echo
    /// of a locally-published event) is projection-silent (D4 single-fire). The
    /// cache-serve path is implicitly on the accepted gate (it only replays
    /// already-stored canonical events).
    pub(in crate::kernel) fn project_accepted_event(&mut self, verified: &crate::store::VerifiedEvent) {
        let raw = verified.raw();
        let author = raw.pubkey.clone();
        let event_id = raw.id.clone();
        let created_at_for_trigger = raw.created_at;

        // (2a) Snapshot the capability caches BEFORE the parser dispatch writes
        // them, kind-agnostically.
        let mailbox_before = self.mailbox_cache().snapshot(&author);
        let dm_before = self.recipient_dm_relays(&author);
        let profile_before = self.profile_lookup().profile(&author);
        // ADR-0057 PR 3 — contacts transition is detected ONLY for the active
        // account: the kernel-owned follow-feed effects (`timeline_authors`
        // rebuild, `sync_follow_feed_interests`, `FollowListChanged`,
        // cache-serve) are active-account-scoped (D4 — arbitrary peers' kind:3
        // must not pollute the registry), exactly as the old `ingest_contacts`
        // active-account gate was. Snapshotting only the active author keeps the
        // common case (a non-active peer's kind:3, or any non-kind:3 event)
        // free of an extra cache read.
        let active_author = self.active_account.as_deref() == Some(author.as_str());
        let contacts_before = if active_author {
            self.contacts_lookup().follows(&author)
        } else {
            None
        };

        // (1) NIP-parser dispatch. D6 — a poisoned dispatcher lock degrades to
        // "no parser fired" (graceful, the store insert already succeeded on the
        // live path / the event is already stored on the cache-serve path).
        if let Ok(d) = self.ingest_dispatcher_slot().read() {
            d.dispatch(verified);
        }

        // (2b) Transition sweep AFTER dispatch.
        // Profile (kind:0) supersession → rev bump.
        let profile_after = self.profile_lookup().profile(&author);
        if profile_before != profile_after {
            self.cached_estimated_store_bytes.set(None);
            self.projection_rev_tracker.source_versions.bump_profiles();
            if !self.event_claims.is_empty() {
                self.projection_rev_tracker
                    .source_versions
                    .bump_claimed_event_content();
            }
        }
        // Mailbox (kind:10002) transition → Nip65Arrived recompile + trace.
        let mailbox_after = self.mailbox_cache().snapshot(&author);
        if mailbox_before != mailbox_after {
            self.on_mailbox_changed(&author, &event_id, created_at_for_trigger);
        }
        // DM-relay (kind:10050) transition → DmRelayListChanged recompile.
        let dm_after = self.recipient_dm_relays(&author);
        if dm_before != dm_after {
            self.on_dm_relays_changed(&author, created_at_for_trigger);
        }
        // Contacts (kind:3) transition for the ACTIVE account → the kernel-owned
        // follow-feed effects. The PARSER (`Kind3Parser`) wrote the capability
        // cache between the before/after snapshots above; the KERNEL owns the
        // planner/lifecycle effects, driven here by the transition signal — NOT
        // inlined into the parser (the `IngestParser` contract: parsers are
        // side-effect-free against kernel state). This is the PR 3 replacement
        // for the deleted `ingest_contacts` arm. `Some(vec![]) != None` matters:
        // a freshly-cleared follow set (a kind:3 with no `p` tags) is a real
        // transition the active account must react to (it WITHDRAWS the prior
        // follow-feed interests).
        if active_author {
            let contacts_after = self.contacts_lookup().follows(&author);
            if contacts_before != contacts_after {
                let follows = contacts_after.unwrap_or_default();
                self.on_active_contacts_changed(&author, follows, created_at_for_trigger);
            }
        }

        // (3) D9-clamped app-observer notify.
        let now_secs = self.now_secs();
        let mut kernel_event = helpers::kernel_event_from_verified(verified);
        kernel_event.created_at = kernel_event.created_at.min(now_secs);
        self.notify_event_observers(&kernel_event);
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
    ///    does not name the kind. M2 migration: this recompile is ALSO what
    ///    re-routes a registered kind:0 profile-claim interest from the
    ///    indexer/app-relay cold-start fallback onto the author's own write
    ///    relays — replacing the deleted `refresh_profile_after_mailbox`
    ///    requested→pending re-queue (which only existed because the bespoke
    ///    profile path was outside the registry chokepoint).
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
