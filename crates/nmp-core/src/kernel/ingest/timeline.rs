//! Kind:1 / kind:6 (note / repost) timeline ingest.
//!
//! Covers event storage, deduplication, timeline ordering, thread hydration
//! queue management, and the seed-timeline open gate.

use super::super::*;
use super::event_short_id;

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
    ) {
        if !self.should_store_event(sub_id, &event) {
            return;
        }

        // D4: route through EventStore for ALL deliveries, including duplicates.
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
                return;
            }
        };
        // T105: provenance is the resolved per-author write relay the EVENT
        // actually arrived on, not the lane's bootstrap URL.
        let provenance = relay_url.to_string();
        let received_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let proceed = match self.store.insert(verified, &provenance, received_at_ms) {
            Ok(outcome) => {
                use crate::store::InsertOutcome;
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
                    InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. } => true,
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
                        return;
                    }
                    InsertOutcome::Superseded { .. } => return,
                    InsertOutcome::Tombstoned { .. }
                    | InsertOutcome::Rejected { .. }
                    | InsertOutcome::Ephemeral { .. } => return,
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
                    return;
                }
                true
            }
        };

        if !proceed {
            return;
        }

        // T82 discovery seam (notedeck §3.10): collect referenced-but-missing
        // pubkeys/event ids (p/e/q tags) into UnknownIds *before* `event.tags`
        // is moved into the cache — borrowed visitor, no clone, zero alloc
        // when every reference is already cached (D8). The actor turns the
        // deduped set into OneshotApi fetches via `drain_unknown_oneshots`.
        self.collect_unknown_refs(&event.tags);

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
            self.diagnostic_firehose_events = self.diagnostic_firehose_events.saturating_add(1);
        }
        self.enqueue_thread_hydration_from_event(&event.id);
        if self.timeline_authors.contains(&event.pubkey) || sub_id.starts_with("diag-firehose-") {
            self.timeline.push_back(event.id);
            self.sort_timeline();
            self.timeline_first_item_at.get_or_insert_with(Instant::now);
        }
        self.changed_since_emit = true;
    }

    pub(in crate::kernel) fn should_store_event(&self, sub_id: &str, event: &NostrEvent) -> bool {
        self.timeline_authors.contains(&event.pubkey)
            || self
                .selected_author
                .as_ref()
                .map(|interest| interest.key == event.pubkey)
                .unwrap_or(false)
            || sub_id.starts_with("author-notes-")
            || sub_id.starts_with("thread-ids-")
            || sub_id.starts_with("thread-replies-")
            || sub_id.starts_with("diag-firehose-")
            // T82/T104: a discovered quoted-note / referenced event arrives on
            // its oneshot sub — it must be stored so the missing reference is
            // actually resolved (otherwise the next ingest re-discovers it).
            // Uses typed OneshotKind dispatch (T104) rather than string-prefix.
            || self.is_discovery_oneshot(sub_id)
    }

    pub(in crate::kernel) fn enqueue_thread_hydration_from_event(&mut self, event_id: &str) {
        let Some(selected) = self
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

    pub(in crate::kernel) fn sort_timeline(&mut self) {
        let mut ids = self.timeline.iter().cloned().collect::<Vec<_>>();
        ids.sort_by(|left, right| {
            let a = self
                .events
                .get(left)
                .map(|event| event.created_at)
                .unwrap_or(0);
            let b = self
                .events
                .get(right)
                .map(|event| event.created_at)
                .unwrap_or(0);
            b.cmp(&a).then_with(|| left.cmp(right))
        });
        ids.truncate(500);
        self.timeline = ids.into();
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
            self.timeline_opened_at = Some(Instant::now());
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
                .map(|deadline| Instant::now() >= deadline)
                .unwrap_or(false)
    }
}
