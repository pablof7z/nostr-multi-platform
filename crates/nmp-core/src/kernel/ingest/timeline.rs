//! Host-declared follow-feed timeline ingest.
//!
//! Covers event storage, deduplication, timeline ordering, and the
//! seed-timeline open gate. (V-112: thread hydration queue management moved
//! app-side with the legacy thread view stack.)

use super::super::{Instant, Kernel, NostrEvent, OutboundMessage, RelayRole, StoredEvent};
use super::helpers::{event_short_id, raw_event_from_nostr, raw_tap_should_fire};

impl Kernel {
    /// Ingest a host-declared follow-feed event into the local read-cache and timeline.
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
            // V-59 rung 1 (Q7) — pre-kind:3 buffer. A host-declared
            // follow-feed event whose author is not (yet) in the active
            // account's follow set would otherwise be dropped here. Park it
            // instead: a later contact-list sync (`sync_follow_feed_interests`)
            // that adds the author replays it.
            //
            // `should_store_event`'s FIRST clause is
            // `timeline_authors.contains(author)`, so reaching this branch
            // already implies `!timeline_authors.contains(author)`; the
            // explicit re-check below is kept for self-documenting intent and
            // to stay correct if that clause is ever reordered. We only buffer
            // only buffer host-declared follow-feed kinds; other kinds dropped
            // here have their own ingest arms and never depend on the follow set.
            if self.follow_feed_kinds.contains(&event.kind)
                && !self.timeline_authors.contains(&event.pubkey)
            {
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
        // Capture the raw event for the `IngestParser` dispatcher fan-out below
        // (after `store.insert` consumes `verified`). The sig is preserved here
        // so `from_store_verified_unchecked` can reconstruct a correct
        // `VerifiedEvent` without re-running Schnorr verification — trust
        // boundary: `try_from_raw` above already verified the signature.
        //
        // D8: clone ONLY when at least one parser is registered for this kind.
        // We read the dispatcher lock once here — a cheap O(parsers) check —
        // and skip the clone entirely when the dispatcher is idle or has no
        // match for this kind. The `Option` is consumed at the dispatch site
        // below (after `Inserted | Replaced` is confirmed).
        let raw_for_dispatch = if self
            .ingest_dispatcher_slot()
            .read()
            .map_or(false, |d| d.is_interested(verified.raw().kind))
        {
            Some(crate::store::RawEvent {
                id: verified.raw().id.clone(),
                pubkey: verified.raw().pubkey.clone(),
                created_at: verified.raw().created_at,
                kind: verified.raw().kind,
                tags: verified.raw().tags.clone(),
                content: verified.raw().content.clone(),
                sig: verified.raw().sig.clone(),
            })
        } else {
            None
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
        // V-56: extend discovery to profile mentions that appear ONLY in
        // event.content (nostr:npub1…/nostr:nprofile1… URIs with no matching
        // p-tag). Must happen before `event.content` is moved into StoredEvent
        // below. D8-clean: the `nostr:` substring guard in
        // `collect_content_mention_pubkeys` short-circuits before any alloc on
        // the common (no-mention) path.
        self.collect_content_mention_pubkeys(&event.content);
        // F-CR-00 capstone: proactive kind:0 fetch removed. The kernel now
        // fetches kind:0 ONLY in response to component claims
        // (`claim_profile` / `claim_event`). Every author-displaying
        // component on all platforms self-claims on mount:
        //   iOS:     ChirpAvatar `.task(id: pubkey)` → claimProfile
        //   Android: RememberProfileClaim (DisposableEffect)
        //   TUI:     claim_visible_author_profile diff
        //   Web:     Post.onMount → claimProfileCommand (#885)
        //   Gallery: claim_profile at render time
        // The `author_display_name` fallback baked into each TimelineItem
        // snapshot is populated from the profile cache as soon as a
        // previously-claimed kind:0 arrives — no blank-out on first render.
        // The `claimed_events` / `resolved_profiles` enrichment reads
        // `self.profiles` which the claim path populates unchanged.

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
        let now_secs = self.now_secs();
        let kernel_event = crate::substrate::KernelEvent {
            id: cached.id.clone(),
            author: cached.author.clone(),
            kind: cached.kind,
            // D9: kernel owns time — clamp relay-supplied created_at to now so a
            // future-dated event from a hostile/buggy relay cannot pin
            // permanently at the top of every consumer's feed. The StoredEvent
            // retains the original timestamp for protocol correctness (NIP-01
            // replaceable/ephemeral handling); only the observer-fan-out shape
            // is clamped.
            created_at: cached.created_at.min(now_secs),
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
        self.cached_estimated_store_bytes.set(None);
        self.notify_event_observers(&kernel_event);
        // V-40 / raw-tap retirement ladder: fan to the substrate
        // `EventIngestDispatcher` so all registered `IngestParser`s receive
        // timeline events (kind:1 notes, kind:6 reposts, and any other
        // host-declared follow-feed kind).
        //
        // Gap closed: before this fix, parsers registered for all-kinds ranges
        // (e.g. `chirp-tui`'s `RawCacheIngestParser`, `0..u32::MAX`) silently
        // missed every timeline event because this call was absent here while
        // being present in `verify_and_persist` (the wildcard ingest arm) and
        // `ingest_pre_verified_event` (the test-support inject path).
        //
        // Gating: Inserted|Replaced only — `Duplicate` returns earlier in this
        // function and never reaches here; `proceed = true` at this point
        // guarantees an Inserted or Replaced outcome. No Ephemeral gate needed:
        // timeline events (kind:1 / kind:6) are never ephemeral.
        //
        // Clone gate: `raw_for_dispatch` is `Some` only when `is_interested`
        // returned `true` above (at least one registered parser covers this
        // kind). When the dispatcher is idle or has no match, both the clone
        // and this second lock acquisition are skipped entirely (D8).
        //
        // D6 — a poisoned dispatcher lock degrades to "no parser fired"; the
        // store insert and observer fan-out already succeeded, so this is the
        // safe graceful-degrade.
        if let Some(raw) = raw_for_dispatch {
            use nmp_store::__nmp_core_internal;
            let verified_for_dispatch = __nmp_core_internal::from_store_verified_unchecked(raw);
            if let Ok(d) = self.ingest_dispatcher_slot().read() {
                d.dispatch(&verified_for_dispatch);
            }
        }
        if sub_id.starts_with("diag-firehose-") {
            self.diagnostic_firehose.events = self.diagnostic_firehose.events.saturating_add(1);
        }
        // V-112 (ADR-0042): enqueue_thread_hydration_from_event call deleted —
        // thread hydration is now handled by the per-app FlatFeed.
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
        // V-112 (ADR-0042): author_view.selected_author clause + author-notes-/
        // thread-ids-/thread-replies- sub_id prefix clauses deleted. These were
        // admission gates for the legacy author_view/thread_view state machine; the
        // FlatFeed seam uses open_interest which is covered by matches_active_open_interest.
        self.timeline_authors.contains(&event.pubkey)
            || sub_id.starts_with("diag-firehose-")
            // T82/T104: a discovered quoted-note / referenced event arrives on
            // its oneshot sub — it must be stored so the missing reference is
            // actually resolved (otherwise the next ingest re-discovers it).
            // Uses typed OneshotKind dispatch (T104) rather than string-prefix.
            || self.is_discovery_oneshot(sub_id)
            || self.claim_expansion_match_author(sub_id, event).is_some()
            // M2 (ADR-0042 §5.1): admit any event matching the wire filter of an
            // active generic `open_interest`. This is the single generalised
            // admission clause that makes a generic `open_interest` REQ
            // functional end-to-end — a non-followed author's notes, an
            // arbitrary thread, or a `#t` hashtag feed reach `self.events` (and
            // thus the `notify_event_observers` feed-engine fan-out) without any
            // bespoke per-view sub-id prefix. The wire sub_id is a *merged*
            // compiler hash (the lattice coalesces many shapes into one REQ), so
            // it cannot be reverse-mapped to one interest; matching the event
            // against the registered shapes is the robust admission test.
            //
            // D8 cost: this walks the active-interest set per inbound event. The
            // cheap `timeline_authors.contains` short-circuit above still fronts
            // the follow-feed hot path (the common case), so the walk only runs
            // for events the follow-set / view / oneshot clauses did not already
            // admit.
            || self.matches_active_open_interest(event)
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
