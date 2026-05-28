//! Kind:1 / kind:6 (note / repost) timeline ingest.
//!
//! Covers event storage, deduplication, timeline ordering, thread hydration
//! queue management, and the seed-timeline open gate.

use super::super::{
    event_references, referenced_event_ids, Instant, Kernel, NostrEvent, OutboundMessage,
    RelayRole, StoredEvent,
};
use super::{event_short_id, raw_event_from_nostr, raw_tap_should_fire};

impl Kernel {
    /// Ingest a kind:1 or kind:6 event into the local read-cache and timeline.
    ///
    /// Routes through `EventStore::insert` (D4 single-writer).  On `Inserted |
    /// Replaced`, populates the lightweight `events` read-cache and appends to
    /// `timeline`.  On `Duplicate`, updates `relay_count` from the authoritative
    /// provenance count in the store.  All other outcomes (Superseded, Tombstoned,
    /// Rejected, Ephemeral) are dropped.
    pub(in crate::kernel) fn ingest_timeline_event(
        &mut self,
        _role: RelayRole,
        relay_url: &str,
        sub_id: &str,
        event: NostrEvent,
    ) -> bool {
        if !self.should_store_event(sub_id, &event) {
            // V-59 rung 1 (Q7) — pre-kind:3 buffer. A kind:1 / kind:6 event
            // whose author is not (yet) in the active account's follow set
            // would otherwise be dropped here. Park it instead: a later kind:3
            // (`sync_follow_feed_interests`) that adds the author replays it.
            //
            // `should_store_event`'s FIRST clause is
            // `timeline_authors.contains(author)`, so reaching this branch
            // already implies `!timeline_authors.contains(author)`; the
            // explicit re-check below is kept for self-documenting intent and
            // to stay correct if that clause is ever reordered. We only buffer
            // note/repost kinds — other kinds dropped here have their own
            // ingest arms and never depend on the follow set.
            if matches!(event.kind, 1 | 6) && !self.timeline_authors.contains(&event.pubkey) {
                self.pre_kind3_buffer
                    .insert(event.id.clone(), (event, relay_url.to_string()));
            }
            return false;
        }

        let mut accepted_for_score = false;

        // D4: route through EventStore for ALL deliveries, including duplicates.
        let verified = match crate::store::VerifiedEvent::try_from_raw(raw_event_from_nostr(&event))
        {
            Ok(v) => v,
            Err(e) => {
                self.log(format!(
                    "sig verify failed for {}: {e}",
                    event_short_id(&event.id)
                ));
                return false;
            }
        };
        let raw_for_observer = if self.raw_event_observers_idle_for_kind(event.kind) {
            None
        } else {
            Some(verified.raw().clone())
        };
        // T105: provenance is the resolved per-author write relay the EVENT
        // actually arrived on, not the lane's bootstrap URL.
        let provenance = relay_url.to_string();
        // Clock seam: `received_at_ms` reads the injected `Clock` via the
        // shared `ingest_received_at_ms` helper (D9 — kernel owns time).
        let received_at_ms = self.ingest_received_at_ms();

        let proceed = match self.store.insert(verified, &provenance, received_at_ms) {
            Ok(outcome) => {
                use crate::store::InsertOutcome;
                if raw_for_observer
                    .as_ref()
                    .is_some_and(|_| raw_tap_should_fire(&outcome))
                {
                    if let Some(raw) = raw_for_observer.as_ref() {
                        self.notify_raw_event_observers(raw, &provenance);
                    }
                }
                // T131 — bump per-URL `RelayUsefulness` counters in the
                // same match arms (design doc §3 line 188: 0 per-event
                // alloc on the hot path; the `provenance` URL is already
                // in scope at line 62 above).
                match &outcome {
                    InsertOutcome::Inserted { .. } => {
                        self.event_provenance
                            .record_first_source(&event.id, &provenance);
                    }
                    InsertOutcome::Replaced { .. } => {
                        self.event_provenance.record_replaced(&provenance);
                    }
                    InsertOutcome::Duplicate { .. } => {
                        self.event_provenance.record_duplicate(&provenance);
                    }
                    InsertOutcome::Rejected { .. } => {
                        self.event_provenance.record_rejected(&provenance);
                    }
                    // Superseded / Tombstoned / Ephemeral are not relay-
                    // usefulness signals — neither novel nor a redundant
                    // copy, they're protocol-state transitions.
                    InsertOutcome::Superseded { .. }
                    | InsertOutcome::Tombstoned { .. }
                    | InsertOutcome::Ephemeral { .. } => {}
                }
                match outcome {
                    InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. } => {
                        accepted_for_score = true;
                        true
                    }
                    InsertOutcome::Duplicate { sources_after, .. } => {
                        if let Some(cached) = self.events.get_mut(&event.id) {
                            // Diagnostic counter: a cached event becomes a
                            // "duplicate" the first time its relay_count
                            // crosses 1 → >1. Subsequent bumps (2→3, …) do
                            // not add a new duplicate event to the count.
                            if cached.relay_count == 1 && sources_after > 1 {
                                self.metric_duplicate_events =
                                    self.metric_duplicate_events.saturating_add(1);
                            }
                            cached.relay_count = sources_after;
                        }
                        return false;
                    }
                    InsertOutcome::Superseded { .. } => return false,
                    InsertOutcome::Tombstoned { .. }
                    | InsertOutcome::Rejected { .. }
                    | InsertOutcome::Ephemeral { .. } => return false,
                }
            }
            Err(e) => {
                self.log(format!("store insert error: {e}"));
                if self.events.contains_key(&event.id) {
                    if let Some(cached) = self.events.get_mut(&event.id) {
                        // Diagnostic counter: count the 1 → >1 transition only
                        // (mirrors the `InsertOutcome::Duplicate` arm above).
                        if cached.relay_count == 1 {
                            self.metric_duplicate_events =
                                self.metric_duplicate_events.saturating_add(1);
                        }
                        cached.relay_count = cached.relay_count.saturating_add(1);
                    }
                    return false;
                }
                true
            }
        };

        if !proceed {
            return false;
        }

        // T82 discovery seam (notedeck §3.10): collect referenced-but-missing
        // pubkeys/event ids (p/e/q tags) into UnknownIds *before* `event.tags`
        // is moved into the cache — borrowed visitor, no clone, zero alloc
        // when every reference is already cached (D8). The actor turns the
        // deduped set into OneshotApi fetches via `drain_unknown_oneshots`.
        self.collect_unknown_refs(&event.tags);
        self.request_profile_for_rendered_note(&event.pubkey);

        let cached = StoredEvent {
            id: event.id.clone(),
            author: event.pubkey.clone(),
            kind: event.kind,
            created_at: event.created_at,
            tags: event.tags,
            content: event.content,
            relay_count: 1,
        };
        // D0 — kernel emits, per-app crates compose. ADR-0009. Build the
        // FFI-stable `KernelEvent` from the freshly-cached `StoredEvent`
        // before either is moved into `self.events` so the fan-out has
        // exactly the same fields the projection would see on snapshot.
        // T146 — observer fan-out fires for every event that reaches the
        // in-memory read-cache; duplicates / supersessions return earlier
        // in this function and never call `notify_event_observers`.
        let kernel_event = crate::substrate::KernelEvent {
            id: cached.id.clone(),
            author: cached.author.clone(),
            kind: cached.kind,
            created_at: cached.created_at,
            tags: cached.tags.clone(),
            content: cached.content.clone(),
        };
        // Diagnostic counters maintained incrementally so `make_update` never
        // walks the whole `events` HashMap to recompute them (60 Hz hot path).
        self.metric_stored_events = self.metric_stored_events.saturating_add(1);
        if cached.kind == 1 {
            self.metric_note_events = self.metric_note_events.saturating_add(1);
        }
        self.events.insert(event.id.clone(), cached);
        self.notify_event_observers(&kernel_event);
        if sub_id.starts_with("diag-firehose-") {
            self.diagnostic_firehose.events = self.diagnostic_firehose.events.saturating_add(1);
        }
        self.enqueue_thread_hydration_from_event(&event.id);
        if self.timeline_authors.contains(&event.pubkey) || sub_id.starts_with("diag-firehose-") {
            self.insert_timeline_id_sorted(event.id);
            self.timing
                .timeline_first_item_at
                .get_or_insert_with(Instant::now);
        }
        self.changed_since_emit = true;
        accepted_for_score
    }

    pub(in crate::kernel) fn should_store_event(&self, sub_id: &str, event: &NostrEvent) -> bool {
        self.timeline_authors.contains(&event.pubkey)
            || self
                .author_view
                .selected_author
                .as_ref()
                .is_some_and(|interest| interest.key == event.pubkey)
            || sub_id.starts_with("author-notes-")
            || sub_id.starts_with("thread-ids-")
            || sub_id.starts_with("thread-replies-")
            || sub_id.starts_with("diag-firehose-")
            // T82/T104: a discovered quoted-note / referenced event arrives on
            // its oneshot sub — it must be stored so the missing reference is
            // actually resolved (otherwise the next ingest re-discovers it).
            // Uses typed OneshotKind dispatch (T104) rather than string-prefix.
            || self.is_discovery_oneshot(sub_id)
            || self.claim_expansion_match_author(sub_id, event).is_some()
    }

    pub(in crate::kernel) fn enqueue_thread_hydration_from_event(&mut self, event_id: &str) {
        let Some(selected) = self
            .thread_view
            .selected_thread
            .as_ref()
            .map(|interest| interest.key.clone())
        else {
            return;
        };
        let Some(event) = self.events.get(event_id).cloned() else {
            return;
        };
        let root = self
            .thread_root_id(&selected)
            .unwrap_or_else(|| selected.clone());
        let is_related = event.id == selected
            || event.id == root
            || event_references(&event, &selected)
            || event_references(&event, &root);
        if !is_related {
            return;
        }

        self.enqueue_thread_reply_target(event.id.clone());
        for id in referenced_event_ids(&event) {
            self.enqueue_thread_id(id.clone());
            self.enqueue_thread_reply_target(id);
        }
    }

    /// T140 — follow-feed open milestone + pending profile-claim flush.
    ///
    /// ## M1 follow-feed REQ emission is RETIRED (T140 cutover)
    ///
    /// This function NO LONGER emits the hand-rolled `seed-timeline-*` REQ.
    /// The follow feed is now carried exclusively by the M2 planner: kind:3
    /// ingest registers per-follow `LogicalInterest`s
    /// (`sync_follow_feed_interests`) and `drain_lifecycle_tick()` (the actor
    /// idle loop) compiles + emits the per-NIP-65-write-relay REQ/CLOSE diff.
    /// The seed-author bootstrap feed is independently covered by
    /// `startup_requests()` (`seed-bootstrap` REQ + seed pubkeys seeded into
    /// `timeline_authors`), so retiring the M1 path here does not regress it.
    ///
    /// `timeline_authors` is single-sourced from the M2 projection
    /// (`sync_follow_feed_interests`) — the divergent `self.timeline_authors =
    /// authors` assignment that previously lived here is deleted so the M1 and
    /// M2 views cannot drift apart.
    ///
    /// The `timeline_requested` / `timeline_opened_at` milestone flags are
    /// still flipped: `status.rs` reports cache-coverage off them, and the
    /// milestone now means "the follow feed has been opened" regardless of
    /// which subsystem carries it.
    ///
    /// Returns only the pending profile-claim requests (UI-driven, unrelated
    /// to the follow feed).
    pub(in crate::kernel) fn maybe_open_timeline(&mut self) -> Vec<OutboundMessage> {
        if !self.timeline_requested && self.should_open_timeline() {
            self.timeline_requested = true;
            self.timing.timeline_opened_at = Some(Instant::now());
            self.log(
                "follow-feed open milestone reached — carried by M2 planner \
                 (drain_lifecycle_tick); M1 seed-timeline-* REQ retired (T140)"
                    .to_string(),
            );
        }

        self.pending_profile_claim_requests()
    }

    pub(in crate::kernel) fn should_open_timeline(&self) -> bool {
        if self.timeline_requested {
            return false;
        }

        let has_active_contacts = self
            .active_account
            .as_ref()
            .and_then(|pk| self.seed_contacts.get(pk))
            .is_some();
        has_active_contacts
            || self
                .contacts_deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
    }
}
