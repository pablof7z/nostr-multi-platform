//! Timeline read-cache projection.
//!
//! ADR-0057 — the timeline read-cache (`self.events` / `self.timeline`) is a
//! **chokepoint-fed projection observer**, not a per-kind ingest arm. Admission
//! + persistence are owned by the single accepted-event chokepoint
//! ([`Kernel::ingest_accepted_event`] → [`Kernel::verify_and_persist`], the D4
//! single-writer); this module only decides whether an already-persisted event
//! belongs in the timeline VIEW (the read-time relevance predicate
//! [`Kernel::should_store_event`]) and, if so, projects it into the read-cache
//! with the D9 created_at clamp. (V-112: thread hydration queue management moved
//! app-side with the legacy thread view stack.)

use super::super::{Instant, Kernel, NostrEvent, OutboundMessage, StoredEvent};

impl Kernel {
    /// TEST-SUPPORT ONLY — historical kind:1|6 timeline-ingest entry point.
    ///
    /// ADR-0057 deleted the production per-kind timeline ingest arm; production
    /// timeline events flow through the single accepted-event chokepoint
    /// ([`Kernel::ingest_accepted_event`]) reached from `handle_event`. This
    /// driver preserves the `(role, relay_url, sub_id, event) -> bool` signature
    /// the existing `nmp-core` test suite drives directly, routing through the
    /// SAME production chokepoint so the tests exercise the real path (no
    /// shadow ingest logic). It then projects through the timeline read-cache
    /// helper directly so tests can exercise timeline behavior without
    /// reintroducing a production follow-feed kind gate.
    ///
    /// Returns `true` iff the store accepted the event as canonical
    /// (`Inserted | Replaced`), mirroring the old method's "stored?" boolean.
    #[cfg(test)]
    pub(in crate::kernel) fn ingest_timeline_event(
        &mut self,
        _role: super::super::RelayRole,
        relay_url: &str,
        sub_id: &str,
        event: NostrEvent,
    ) -> bool {
        let outcome = self.ingest_accepted_event(
            super::IngestSource::Relay { relay_url, sub_id },
            event.clone(),
        );
        self.project_timeline_event(sub_id, &event, outcome.as_ref());
        matches!(
            outcome,
            Some(
                crate::store::InsertOutcome::Inserted { .. }
                    | crate::store::InsertOutcome::Replaced { .. }
            )
        )
    }

    /// ADR-0057 — project an already-persisted timeline event
    /// into the timeline read-cache (`self.events` + `self.timeline`).
    ///
    /// Called by the test/diagnostic timeline path AFTER `verify_and_persist`
    /// has run the authoritative `store.insert` (D4 single-writer), the
    /// NIP-parser dispatch, and the app-observer notify. This method does
    /// **no** sig-verify and **no** `store.insert` — persistence already
    /// happened, gated only by valid signature, kind-agnostically.
    ///
    /// - On `Duplicate` (a sibling-relay re-delivery, incl. the relay echo of a
    ///   locally-published event): bump the cached `relay_count` from the
    ///   authoritative store count — a diagnostic signal, NOT a projection
    ///   mutation (D4: observers already fired exactly once in the chokepoint).
    ///   Then return; projection-silent.
    /// - On `Inserted | Replaced`: if the read-time relevance predicate
    ///   [`Kernel::should_store_event`] admits the event into the timeline VIEW,
    ///   populate the `events` read-cache (with the D9 clamp) and append to
    ///   `timeline`. A non-relevant (e.g. non-followed) event is still persisted
    ///   in the store — it just does not enter the timeline projection.
    /// - All other outcomes (and sig-verify failure / `None`) are no-ops here.
    pub(in crate::kernel) fn project_timeline_event(
        &mut self,
        sub_id: &str,
        event: &NostrEvent,
        outcome: Option<&crate::store::InsertOutcome>,
    ) {
        use crate::store::InsertOutcome;

        match outcome {
            Some(InsertOutcome::Duplicate { sources_after, .. }) => {
                // ADR-0057 — the `relay_count` bump on `Duplicate` is preserved
                // (a diagnostic signal). It stays projection-silent: the
                // observers already fired once on the original `Inserted`.
                if let Some(cached) = self.events.get_mut(&event.id) {
                    if cached.relay_count == 1 && *sources_after > 1 {
                        self.metric_duplicate_events =
                            self.metric_duplicate_events.saturating_add(1);
                    }
                    cached.relay_count = *sources_after;
                }
                return;
            }
            Some(InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. }) => {}
            // Superseded / Tombstoned / Rejected / Ephemeral / sig-verify
            // failure (None): no timeline projection.
            _ => return,
        }

        // Read-time relevance predicate (ADR-0057): "does this event belong in
        // MY timeline VIEW?". It NO LONGER gates persistence — the event is
        // already in the authoritative store. A non-relevant event is simply
        // absent from the timeline read-cache; a later follow (kind:3) that adds
        // the author surfaces the event from the store on the next cache-serve
        // (the deleted `pre_kind3_buffer` is obsolete now admission ≠
        // persistence — there is no event to "park", it is already persisted).
        if !self.should_store_event(sub_id, event) {
            return;
        }

        // T82 discovery seam (notedeck §3.10): collect referenced-but-missing
        // pubkeys/event ids (p/e/q tags) into UnknownIds. The actor turns the
        // deduped set into OneshotApi fetches via `drain_unknown_oneshots`.
        self.collect_unknown_refs(&event.tags);
        // V-56: extend discovery to profile mentions that appear ONLY in
        // event.content (nostr:npub1…/nostr:nprofile1… URIs with no matching
        // p-tag). D8-clean: the `nostr:` substring guard in
        // `collect_content_mention_pubkeys` short-circuits before any alloc on
        // the common (no-mention) path.
        self.collect_content_mention_pubkeys(&event.content);
        // F-CR-00 capstone: proactive kind:0 fetch removed. The kernel now
        // fetches kind:0 ONLY in response to component claims
        // (`resolve_ref`). Every author-displaying
        // component on all platforms self-claims on mount:
        //   iOS:     ChirpAvatar `.task(id: pubkey)` → claimProfile
        //   Android: RememberProfileClaim (DisposableEffect)
        //   TUI:     claim_visible_author_profile diff
        //   Web:     Post.onMount → claimProfileCommand (#885)
        //   Gallery: resolve_ref at render time
        // Profile rendering now flows through explicit profile claims and
        // `refs.profile` materialization. `refs.event` stays raw and carries the
        // author pubkey for profile components to compose.

        // D9: kernel owns time — clamp relay-supplied created_at to now so a
        // future-dated event from a hostile/buggy relay cannot pin permanently
        // at the top of the timeline VIEW. ADR-0057: the clamp is applied to the
        // observer-delivered `KernelEvent` at the chokepoint (so ALL feed
        // consumers are protected), and this read-cache projection clamps
        // independently as well — strictly stronger, since it also clamps the
        // kernel's own timeline ordering in `timeline_order.rs`. The
        // authoritative `EventStore` row retains the original wire timestamp for
        // protocol correctness (NIP-01 replaceable/ephemeral handling).
        let now_secs = self.now_secs();
        let cached = StoredEvent {
            id: event.id.clone(),
            author: event.pubkey.clone(),
            kind: event.kind,
            created_at: event.created_at.min(now_secs),
            tags: event.tags.clone(),
            content: event.content.clone(),
            relay_count: 1,
        };
        // Diagnostic counters maintained incrementally so `make_update` never
        // walks the whole `events` HashMap to recompute them (60 Hz hot path).
        self.metric_stored_events = self.metric_stored_events.saturating_add(1);
        if cached.kind == 1 {
            self.metric_note_events = self.metric_note_events.saturating_add(1);
        }
        self.events.insert(event.id.clone(), cached);
        self.cached_estimated_store_bytes.set(None);
        if sub_id.starts_with("diag-firehose-") {
            self.diagnostic_firehose.events = self.diagnostic_firehose.events.saturating_add(1);
        }
        // V-112 (ADR-0042): enqueue_thread_hydration_from_event call deleted —
        // thread hydration is now handled by the per-app FlatFeed.
        if self.timeline_authors.contains(&event.pubkey) || sub_id.starts_with("diag-firehose-") {
            self.insert_timeline_id_sorted(event.id.clone());
            self.timing
                .timeline_first_item_at
                .get_or_insert_with(Instant::now); // doctrine-allow: D9 — timeline diagnostic elapsed-time marker; not replay policy
        }
        self.changed_since_emit = true;
    }

    /// ADR-0057 — TIMELINE-PROJECTION (read-time VIEW) predicate. "Does this
    /// already-persisted event belong in MY timeline VIEW?".
    ///
    /// This has **no** power over persistence. Persistence is owned by the
    /// chokepoint ([`Kernel::verify_and_persist`], the D4 single-writer) and is
    /// gated only by a valid signature — kind-agnostically, relevance-blind. By
    /// the time this predicate runs, the event is ALREADY in the authoritative
    /// store. A `false` here means only "do not project this into the timeline
    /// read-cache"; the event remains in the store and is surfaced later by
    /// cache-serve if a follow / interest brings it into view. Do NOT
    /// reintroduce a `store.insert` gate on this predicate — that was the #1442
    /// relevance-shaped-holes bug ADR-0057 removed.
    pub(in crate::kernel) fn should_store_event(&self, sub_id: &str, event: &NostrEvent) -> bool {
        // V-112 (ADR-0042): author_view.selected_author clause + author-notes-/
        // thread-ids-/thread-replies- sub_id prefix clauses deleted. These were
        // view predicates for the legacy author_view/thread_view state machine; the
        // FlatFeed seam uses open_interest which is covered by matches_active_open_interest.
        self.timeline_authors.contains(&event.pubkey)
            || sub_id.starts_with("diag-firehose-")
            // T82/T104: a discovered quoted-note / referenced event arrives on
            // its oneshot sub — it belongs in the view that requested it so the
            // missing reference resolves. (It is already persisted regardless;
            // this clause only governs timeline-VIEW membership.) Uses typed
            // OneshotKind dispatch (T104) rather than string-prefix.
            || self.is_discovery_oneshot(sub_id)
            || self.claim_expansion_match_author(sub_id, event).is_some()
            // M2 (ADR-0042 §5.1): include any event matching the wire filter of
            // an active generic `open_interest` in the timeline VIEW. This is the
            // single generalised view clause that makes a generic `open_interest`
            // REQ render end-to-end — a non-followed author's notes, an arbitrary
            // thread, or a `#t` hashtag feed reach the timeline read-cache
            // (`self.events`) without any bespoke per-view sub-id prefix.
            // (`notify_event_observers` is NOT gated by this clause — the
            // chokepoint fires observers unconditionally on Inserted|Replaced|
            // Ephemeral per ADR-0057; this clause governs timeline-VIEW
            // membership only.) The wire sub_id is a *merged* compiler hash
            // (the lattice coalesces many shapes into one REQ), so it cannot be
            // reverse-mapped to one interest; matching the event against the
            // registered shapes is the robust view test.
            //
            // D8 cost: this walks the active-interest set per inbound event. The
            // cheap `timeline_authors.contains` short-circuit above still fronts
            // the follow-feed hot path (the common case), so the walk only runs
            // for events the follow-set / view / oneshot clauses did not already
            // include.
            || self.matches_active_open_interest(event)
    }

    /// Cache an accepted non-timeline event when an active generic interest
    /// owns it, so cache-serve wakeup replays dedup against the same projection
    /// fact the live observer fan-out just exposed.
    pub(in crate::kernel) fn cache_event_for_matching_open_interest(
        &mut self,
        event: &NostrEvent,
        relay_count: u32,
    ) {
        if self.events.contains_key(&event.id) || !self.matches_active_open_interest(event) {
            return;
        }

        let cached = StoredEvent {
            id: event.id.clone(),
            author: event.pubkey.clone(),
            kind: event.kind,
            created_at: event.created_at,
            tags: event.tags.clone(),
            content: event.content.clone(),
            relay_count,
        };
        self.metric_stored_events = self.metric_stored_events.saturating_add(1);
        if cached.kind == 1 {
            self.metric_note_events = self.metric_note_events.saturating_add(1);
        }
        self.events.insert(cached.id.clone(), cached);
        self.cached_estimated_store_bytes.set(None);
    }

    /// ADR-0042 §5.1 — does `event` satisfy the wire filter of any active
    /// registered interest? Drives the generalised `should_store_event`
    /// admission clause for generic `open_interest` feeds.
    fn matches_active_open_interest(&self, event: &NostrEvent) -> bool {
        self.lifecycle
            .registry()
            .iter_active()
            .iter()
            .any(|interest| {
                interest.shape.matches_event_with_id(
                    &event.id,
                    &event.pubkey,
                    event.kind,
                    event.created_at,
                    &event.tags,
                )
            })
    }

    pub(in crate::kernel) fn maybe_open_timeline_at(
        &mut self,
        now: Instant,
    ) -> Vec<OutboundMessage> {
        if !self.timeline_requested && self.should_open_timeline(now) {
            self.timeline_requested = true;
            self.timing.timeline_opened_at = Some(now);
            self.log(
                "timeline open milestone reached; acquisition is feed-session owned".to_string(),
            );
        }

        Vec::new()
    }

    pub(in crate::kernel) fn should_open_timeline(&self, now: Instant) -> bool {
        if self.timeline_requested {
            return false;
        }

        // ADR-0057 PR 3 — read the active account's contacts presence from the
        // capability-owned cache (`Arc<dyn ContactsLookup>`) rather than the
        // deleted kernel-owned `seed_contacts` HashMap. `Some(_)` (incl. a
        // cleared `Some(vec![])`) means a kind:3 has arrived for the active
        // account, so the timeline can open.
        let has_active_contacts = self
            .active_account
            .as_deref()
            .map(|pk| self.contacts_lookup().follows(pk).is_some())
            .unwrap_or(false);
        has_active_contacts
            || self
                .contacts_deadline
                .is_some_and(|deadline| now >= deadline)
    }
}
