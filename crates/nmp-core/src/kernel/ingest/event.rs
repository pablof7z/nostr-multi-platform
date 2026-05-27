//! EVENT-frame handling extracted from `ingest/mod.rs`.
//!
//! Contains `handle_event` (the per-kind dispatch hub), `verify_and_persist`
//! (the shared store-insertion path), `ingest_received_at_ms` (the clock
//! seam for store provenance timestamps), `on_mailbox_changed` (the
//! substrate-honest mailbox-change observer), and `kernel_event_from_nostr`
//! (the wire-to-substrate projection).
//!
//! # Extraction rationale
//!
//! `ingest/mod.rs` was over the 500-LOC hard cap (AGENTS.md). Moving the
//! ~300-LOC EVENT block here (together with `eose.rs`) brings `mod.rs`
//! back under the cap.

use super::super::{CanonicalRelayUrl, Instant, Kernel, NostrEvent, RelayRole};
use super::{event_short_id, raw_event_from_nostr, raw_tap_should_fire};
use serde_json::Value;

impl Kernel {
    pub(in crate::kernel) fn handle_event(
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
                        self.record_claim_expansion_hit(sub_id, relay_url, author, &event.id);
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
                        // W8b: `event_id_for_trace` was captured above before
                        // `verify_and_persist` borrowed `event`.
                        self.record_claim_expansion_hit(
                            sub_id,
                            relay_url,
                            author,
                            &event_id_for_trace,
                        );
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
    pub(in crate::kernel) fn verify_and_persist(
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
            super::super::mailboxes::BootstrapSeed::Discovery,
        );
        self.lifecycle
            .enqueue_trigger(crate::subs::CompileTrigger::Nip65Arrived {
                pubkey: author.to_string(),
                created_at,
            });
        self.refresh_profile_after_mailbox(author);
    }
}

pub(super) fn kernel_event_from_nostr(event: &NostrEvent) -> crate::substrate::KernelEvent {
    crate::substrate::KernelEvent {
        id: event.id.clone(),
        author: event.pubkey.clone(),
        kind: event.kind,
        created_at: event.created_at,
        tags: event.tags.clone(),
        content: event.content.clone(),
    }
}
