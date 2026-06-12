//! Test-support helpers for the kernel.
//!
//! All items in this file are gated on `cfg(any(test, feature = "test-support"))`.
//! They provide fast, signature-verification-free injection paths that let
//! unit tests and the firehose/FFI stress harnesses exercise the same ingest
//! hot-paths as production code without needing real secp256k1 keys.
//!
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::relay::RelayRoleTestExt;

thread_local! {
    static CLAIM_EXPANSION_SUBS: RefCell<BTreeMap<String, String>> =
        RefCell::new(BTreeMap::new());
    static CLAIM_EXPANSION_MATCHES: RefCell<BTreeSet<(String, String)>> =
        RefCell::new(BTreeSet::new());
}

pub(crate) fn register_claim_expansion_sub(sub_id: &str, author: &str) {
    CLAIM_EXPANSION_SUBS.with(|m| {
        m.borrow_mut()
            .insert(sub_id.to_string(), author.to_string());
    });
}

pub(crate) fn get_claim_expansion_author(sub_id: &str) -> Option<String> {
    CLAIM_EXPANSION_SUBS.with(|m| m.borrow().get(sub_id).cloned())
}

pub(crate) fn mark_claim_expansion_match_seen(sub_id: &str, relay_url: &str) {
    CLAIM_EXPANSION_MATCHES.with(|m| {
        m.borrow_mut().insert((
            sub_id.to_string(),
            CanonicalRelayUrl::parse_or_raw(relay_url).into_string(),
        ));
    });
}

pub(crate) fn take_claim_expansion_match_seen(sub_id: &str, relay_url: &str) -> bool {
    CLAIM_EXPANSION_MATCHES.with(|m| {
        m.borrow_mut().remove(&(
            sub_id.to_string(),
            CanonicalRelayUrl::parse_or_raw(relay_url).into_string(),
        ))
    })
}

pub(crate) fn clear_claim_expansion_subs() {
    CLAIM_EXPANSION_SUBS.with(|m| m.borrow_mut().clear());
    CLAIM_EXPANSION_MATCHES.with(|m| m.borrow_mut().clear());
}

impl Kernel {
    /// Test-support constructor for downstream protocol crates.
    #[must_use]
    pub fn testing_new(visible_limit: usize) -> Self {
        Self::new(visible_limit)
    }

    /// Deliver a replaceable event (kind:0, 3, or 10002) to the kernel,
    /// bypassing signature verification.
    ///
    /// Mirrors the production `handle_event` dispatch for replaceable kinds but
    /// uses `VerifiedEvent::from_raw_unchecked` so unit tests don't need real
    /// secp256k1 signatures.  Returns the `InsertOutcome` so callers can assert
    /// on supersession behaviour.
    ///
    /// Test-support only — gated on `cfg(any(test, feature = "test-support"))`.
    #[allow(clippy::too_many_arguments, dead_code)]
    pub(crate) fn inject_replaceable_event(
        &mut self,
        id: &str,
        pubkey: &str,
        created_at: u64,
        kind: u32,
        tags: Vec<Vec<String>>,
        relay_url: &str,
        received_at_ms: u64,
    ) -> Option<crate::store::InsertOutcome> {
        use crate::store::{InsertOutcome, RawEvent, VerifiedEvent};
        let raw = RawEvent {
            id: id.to_string(),
            pubkey: pubkey.to_string(),
            created_at,
            kind,
            tags: tags.clone(),
            content: String::new(),
            sig: "a".repeat(128),
        };
        let verified = VerifiedEvent::from_raw_unchecked(raw);
        let outcome = match self
            .store
            .insert(verified, &relay_url.to_string(), received_at_ms)
        {
            Ok(o) => o,
            Err(_) => return None,
        };
        if matches!(
            outcome,
            InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. }
        ) {
            let event = NostrEvent {
                id: id.to_string(),
                pubkey: pubkey.to_string(),
                created_at,
                kind,
                tags,
                content: String::new(),
                sig: "a".repeat(128),
            };
            match kind {
                0 => self.ingest_profile(event),
                3 => self.ingest_contacts(event),
                10002 => {
                    // 2026-05-25: the kernel-side `ingest_relay_list` impl
                    // was deleted alongside the production `10002 =>` arm
                    // (the substrate parser `nmp_router::Kind10002Parser`
                    // is the production writer). Tests cannot wire the
                    // production parser through this helper (which bypasses
                    // `verify_and_persist` and therefore the dispatcher),
                    // so substitute the parser's effect inline:
                    //   1. parse `r` tags into a `ParsedRelayList` (the
                    //      legacy adapter; identical shape to what the
                    //      parser produces);
                    //   2. upsert (or remove on empty) into the substrate
                    //      `MailboxCache` the test kernel owns;
                    //   3. enqueue the `Nip65Arrived` recompile trigger —
                    //      what the kernel's substrate-honest mailbox-change
                    //      observer (`Kernel::on_mailbox_changed`) does in
                    //      production.
                    let parsed =
                        parse_relay_list_to_substrate(&event.id, event.created_at, &event.tags);
                    let empty =
                        parsed.read.is_empty() && parsed.write.is_empty() && parsed.both.is_empty();
                    let had_entry = self.mailbox_cache.known(&event.pubkey);
                    let mailbox_mutated = if empty {
                        if had_entry {
                            self.mailbox_cache.remove(&event.pubkey);
                            self.lifecycle.enqueue_trigger(
                                crate::subs::CompileTrigger::Nip65Arrived {
                                    pubkey: event.pubkey.clone(),
                                    created_at: event.created_at,
                                },
                            );
                            true
                        } else {
                            false
                        }
                    } else {
                        self.mailbox_cache.upsert(event.pubkey.clone(), parsed);
                        self.lifecycle
                            .enqueue_trigger(crate::subs::CompileTrigger::Nip65Arrived {
                                pubkey: event.pubkey.clone(),
                                created_at: event.created_at,
                            });
                        true
                    };
                    // Mirror the production `Kernel::on_mailbox_changed`
                    // profile re-fetch (production: ingest/mod.rs wildcard
                    // arm). This helper bypasses `verify_and_persist` and
                    // therefore skips the production observer — without the
                    // explicit call here every test driven through
                    // `inject_replaceable_event(.., 10002, ..)` would
                    // silently miss the Gap-2 re-fetch.
                    if mailbox_mutated {
                        self.refresh_profile_after_mailbox(&event.pubkey);
                    }
                    self.changed_since_emit = true;
                }
                // V-40: kind:10050 no longer has a kernel-side ingest arm —
                // it routes through the substrate `EventIngestDispatcher`
                // inside `verify_and_persist` above (which this helper
                // already calls). A registered `Kind10050Parser` writes the
                // DM-relay cache.
                _ => {}
            }
        }
        Some(outcome)
    }

    /// Ingest a pre-verified event through the kernel ingest path.
    ///
    /// This method does NOT call `ingest_timeline_event`.  Instead it:
    /// 1. Calls `store.insert` via `from_raw_unchecked` to let the store record
    ///    provenance (D4: store is the single authoritative writer; re-wrap avoids
    ///    redundant re-verification).
    /// 2. Populates the lightweight read-cache (`self.events` HashMap + appends to
    ///    `self.timeline`) directly, mirroring the `Inserted/Replaced` branch of
    ///    `ingest_timeline_event` but without signature re-verification overhead.
    ///
    /// Sort is deferred: callers injecting a batch MUST call
    /// `sort_timeline_deferred()` once after the loop to avoid O(n²·log n) cost.
    ///
    /// D0: capability boundary respected — this method is gated behind
    /// `cfg(any(test, feature = "test-support"))` and is never part of the
    /// production FFI surface.
    pub(crate) fn ingest_pre_verified_event(
        &mut self,
        role: crate::relay::RelayRole,
        sub_id: &str,
        verified: crate::store::VerifiedEvent,
    ) {
        use crate::store::InsertOutcome;

        let raw = verified.into_raw();
        let relay_url = role.url().to_string();
        let received_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        // Re-wrap as VerifiedEvent for the store; from_raw_unchecked is used
        // here because the caller has already verified (or intentionally
        // bypassed) verification.  The store is the single authoritative writer
        // per D4.
        let verified_for_store = crate::store::VerifiedEvent::from_raw_unchecked(raw.clone());

        let proceed = match self
            .store
            .insert(verified_for_store, &relay_url, received_at_ms)
        {
            Ok(outcome) => matches!(
                outcome,
                InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. }
            ),
            Err(e) => {
                self.log(format!("test ingest store error: {e}"));
                !self.events.contains_key(&raw.id)
            }
        };

        if !proceed {
            return;
        }

        let id = raw.id.clone();
        let cached = StoredEvent {
            id: raw.id.clone(),
            author: raw.pubkey.clone(),
            kind: raw.kind,
            created_at: raw.created_at,
            tags: raw.tags.clone(),
            content: raw.content.clone(),
            relay_count: 1,
        };
        // T146 — fan out to registered event observers. Mirrors the
        // production path in `ingest/timeline.rs`. Per-app projections
        // (e.g. `Nip10ModularTimelineView` in `nmp-app-chirp`) ingest the
        // same KernelEvents through the test-support path as production
        // (D0 — kernel emits, per-app crates compose).
        let kernel_event = crate::substrate::KernelEvent {
            id: cached.id.clone(),
            author: cached.author.clone(),
            kind: cached.kind,
            created_at: cached.created_at,
            tags: cached.tags.clone(),
            content: cached.content.clone(),
        };
        // Mirror the production ingest path's incremental diagnostic counters
        // so the test-support inject path keeps `metric_*` in sync with
        // `events` (the snapshot would otherwise drift under test harnesses).
        self.metric_stored_events = self.metric_stored_events.saturating_add(1);
        if cached.kind == 1 {
            self.metric_note_events = self.metric_note_events.saturating_add(1);
        }
        self.events.insert(id.clone(), cached);
        self.notify_event_observers(&kernel_event);
        // Also fan out to raw-event observers (verbatim-signed-event tap for
        // live-delivery consumers such as chirp-tui / hl nostrdb mirror).
        // Mirrors the `verify_and_persist` branch in `ingest/mod.rs` that calls
        // `notify_raw_event_observers` when the store outcome is Inserted|Replaced.
        // The `proceed` gate above already enforces that same store-outcome condition.
        if !self.raw_event_observers_idle_for_kind(raw.kind) {
            self.notify_raw_event_observers(&raw, &relay_url);
        }
        // Fan to the substrate `EventIngestDispatcher` — keeps TEST-SUPPORT
        // injection consistent with the production fan-out in `verify_and_persist`
        // (the wildcard ingest arm) and `ingest_timeline_event` (the follow-feed
        // path).
        //
        // IMPORTANT: `ingest_pre_verified_event` is TEST-SUPPORT ONLY. The FFI
        // symbol `nmp_app_inject_signed_event_json`, the
        // `ActorCommand::IngestPreVerifiedEvents` dispatch arm, and this function
        // are all gated on `#[cfg(any(test, feature = "test-support"))]`. Production
        // ingest flows through `verify_and_persist` and `ingest_timeline_event`,
        // which each call the dispatcher directly. This call mirrors those paths
        // so that tests exercising the inject path see the same IngestParser
        // fan-out as production ingest paths.
        //
        // Gating matches `verify_and_persist`: only fire on Inserted|Replaced
        // (i.e. when `proceed` is true). Duplicate re-deliveries do not re-fire
        // the parser (D4 dedup); the `proceed` check above enforces this.
        //
        // Ephemeral gate divergence: pre-verified injection never carries
        // ephemeral kinds (ephemeral events expire at the relay boundary and
        // are not stored). `verify_and_persist` fires the dispatcher for
        // Ephemeral outcomes; this path does not (the `proceed` gate above
        // already ensures Inserted|Replaced only). If ephemeral pre-verified
        // injection is ever added, the gate here must be re-evaluated.
        //
        // D6 — a poisoned dispatcher lock degrades to "no parser fired"; the
        // store insert already succeeded so this is the safe graceful-degrade.
        {
            let verified_for_dispatch =
                crate::store::VerifiedEvent::from_raw_unchecked(raw.clone());
            if let Ok(d) = self.ingest_dispatcher_slot().read() {
                d.dispatch(&verified_for_dispatch);
            }
        }
        // diag-firehose-stress sub_id: always appended to timeline.
        // sort_timeline() is NOT called here; callers that inject a batch of
        // events must call kernel.sort_timeline_deferred() once after the loop
        // to avoid O(n²·log n) sort overhead for large batches.
        if sub_id.starts_with("diag-firehose-") {
            self.diagnostic_firehose.events = self.diagnostic_firehose.events.saturating_add(1);
            self.timeline.push_back(id);
        }
        self.events_since_last_update = self.events_since_last_update.saturating_add(1);
        self.changed_since_emit = true;
    }

    /// Seed a fully-formed kind:1 note into the kernel's read-cache (`events`).
    ///
    /// Used by the reaction / thread tests in `actor/commands/tests.rs` to
    /// stage a parent note so a subsequent `react(..., target_id)` resolves
    /// the parent author from the read-cache (`event_author`) rather than the
    /// uncached fallback. Bypasses the store entirely — purely a read-cache
    /// fixture. The `tags` argument can carry whatever NIP-10 structure the
    /// test needs.
    #[allow(dead_code)]
    pub(crate) fn seed_kind1_for_reply_test(
        &mut self,
        id: &str,
        author: &str,
        created_at: u64,
        tags: Vec<Vec<String>>,
        content: &str,
    ) {
        self.events.insert(
            id.to_string(),
            StoredEvent {
                id: id.to_string(),
                author: author.to_string(),
                kind: 1,
                created_at,
                tags,
                content: content.to_string(),
                relay_count: 1,
            },
        );
        // Keep the incremental diagnostic counters in sync with `events`
        // (this fixture inserts a kind:1 note directly into the read-cache).
        self.metric_stored_events = self.metric_stored_events.saturating_add(1);
        self.metric_note_events = self.metric_note_events.saturating_add(1);
    }

    // V-112 (ADR-0042): is_thread_hydration_requested() deleted —
    // ThreadViewState (including pending_ids / requested_ids) removed from kernel.

    /// Seed a kind:10002 (NIP-65 relay list) into the kernel's event store and
    /// relay-list cache for `author_pubkey` with `write_urls` as its write-marker
    /// relay tags.
    ///
    /// Required by tests that exercise the publish path after
    /// T-publish-resolver-indexer (codex f81f735): `Nip65OutboxResolver` is now
    /// fail-closed — an author with no kind:10002 resolves to an empty relay set
    /// and the engine returns `NoTargets`. Tests that assert non-empty outbound
    /// frames MUST call this before any publish command.
    ///
    /// Test-support only — gated on `cfg(any(test, feature = "test-support"))`.
    #[allow(dead_code)]
    pub(crate) fn seed_kind10002_for_test(&mut self, author_pubkey: &str, write_urls: &[&str]) {
        // Use the author's pubkey as the synthetic event ID — guaranteed
        // unique per author in a fresh-kernel test. The old two-char prefix
        // approach caused a Duplicate hit when the randomly-generated active
        // pubkey started with the same two hex chars as FIATJAF_HEX ("3b")
        // or SEED_NPUB_HEX ("fa"), making the store return Duplicate and
        // silently skip ingest_relay_list for that author.
        let id = author_pubkey.to_string();
        let tags: Vec<Vec<String>> = write_urls
            .iter()
            .map(|url| vec!["r".to_string(), url.to_string(), "write".to_string()])
            .collect();
        // Use a far-future `created_at` so the seeded relay list always wins the
        // replaceable-event dedup in `store::insert` (strict `>` on `created_at`).
        // `create_account` now caches an onboarding kind:10002 stamped with
        // `Timestamp::now()` (~2026); a fixed past timestamp would lose that race
        // and the seeded list would be silently discarded. `u64::MAX` guarantees
        // the test seed overrides whatever production state was cached.
        self.inject_replaceable_event(
            &id,
            author_pubkey,
            u64::MAX,
            10002,
            tags,
            "wss://seed",
            1_700_000_000_000,
        );
    }

    /// Lazily install (and return) the test-only
    /// [`crate::substrate::TestDmInboxRelayCache`] behind the kernel's
    /// `dm_inbox_relays` slot. First call installs a fresh cache;
    /// subsequent calls return the same `Arc` so seeds compose.
    ///
    /// Test-support only — production composition installs
    /// `nmp_nip17::DmRelayCache` via
    /// [`Kernel::set_dm_inbox_relay_lookup`] instead.
    #[allow(dead_code)]
    pub(crate) fn test_dm_relay_cache(
        &mut self,
    ) -> std::sync::Arc<crate::substrate::TestDmInboxRelayCache> {
        if let Some(cache) = self.test_dm_inbox_cache.as_ref() {
            return std::sync::Arc::clone(cache);
        }
        let cache = std::sync::Arc::new(crate::substrate::TestDmInboxRelayCache::new());
        self.test_dm_inbox_cache = Some(std::sync::Arc::clone(&cache));
        self.set_dm_inbox_relay_lookup(std::sync::Arc::clone(&cache)
            as std::sync::Arc<dyn crate::substrate::DmInboxRelayLookup>);
        cache
    }

    /// Seed `author_pubkey`'s DM-inbox relay list (post-V-40, this writes
    /// to the substrate [`crate::substrate::DmInboxRelayLookup`] handle
    /// rather than to a kernel-owned HashMap — see V-40 in
    /// `docs/architecture/crate-boundaries.md`).
    ///
    /// Production composition installs `nmp_nip17::DmRelayCache` via
    /// [`Kernel::set_dm_inbox_relay_lookup`]; tests inside `nmp-core` use
    /// the [`crate::substrate::TestDmInboxRelayCache`] stand-in (lazily
    /// installed on first call via [`Kernel::test_dm_relay_cache`]).
    /// Repeated calls re-use the same cache, so multi-pubkey seeds compose.
    ///
    /// Also enqueues an [`crate::subs::CompileTrigger::InvalidateCompile`]
    /// on the kernel's `SubscriptionLifecycle` so the planner re-routes
    /// `#p`-tagged DM-inbox interests on the next `drain_lifecycle_tick`
    /// — mirroring the pre-V-40 behaviour where `ingest_dm_relay_list`
    /// enqueued a `DmRelayListChanged` trigger inline.
    ///
    /// Test-support only — gated on `cfg(any(test, feature = "test-support"))`.
    #[allow(dead_code)]
    pub(crate) fn seed_kind10050_for_test(&mut self, author_pubkey: &str, dm_relay_urls: &[&str]) {
        self.test_dm_relay_cache()
            .upsert(author_pubkey, dm_relay_urls);
        // V-40 substitute for the removed `CompileTrigger::DmRelayListChanged`.
        // Production composition (`Kind10050Parser` in `nmp-nip17`) will need
        // its own seam to enqueue a trigger when the cache mutates; for tests
        // we drive it directly here so the planner re-routes on the next tick.
        self.lifecycle
            .enqueue_trigger(crate::subs::CompileTrigger::InvalidateCompile {
                reason: crate::subs::InvalidateReason::External(
                    "test-support: seed_kind10050_for_test".to_string(),
                ),
            });
    }

    /// Sort the timeline once after a batch inject (deferred sort).
    ///
    /// Call this after a loop of `ingest_pre_verified_event` calls to amortize
    /// the O(n log n) sort cost across the whole batch rather than paying it
    /// per-event.
    pub(crate) fn sort_timeline_deferred(&mut self) {
        self.sort_timeline();
    }

    // ─── T140 fix-forward test accessors ─────────────────────────────────────
    // These are only ever called from #[cfg(test)] modules within nmp-core.
    // The test-support feature exposes the rest of this module to downstream
    // crates, but these kernel-internal accessors are not part of that surface.

    /// Mirror the actor wiring: register planner `WireFrame`s into the kernel's
    /// `wire_subs` / persistent-sub bookkeeping. Production path is
    /// `actor::outbound::wire_frames_to_outbound`; tests drive it directly so
    /// the EOSE keep-live assertion exercises the same registration code.
    #[cfg(test)]
    pub(crate) fn register_wire_frames_for_test(&mut self, frames: &[crate::subs::WireFrame]) {
        self.register_planner_wire_frames(frames);
    }

    /// Diagnostic `state` of the wire sub tracked for `(relay_url, sub_id)`,
    /// or `None` if no row exists. #170: relay-scoped key — the same `sub_id`
    /// may legitimately be live on multiple relay connections.
    #[cfg(test)]
    pub(crate) fn wire_sub_state_for_test_on_relay(
        &self,
        relay_url: &str,
        sub_id: &str,
    ) -> Option<String> {
        // T-relay-url-normalize: `wire_subs` is keyed by the canonical relay
        // URL (the planner boundary and the EOSE handler both canonicalize).
        // Canonicalize the query so a test may pass any URL spelling.
        let key = crate::relay::CanonicalRelayUrl::parse_or_raw(relay_url);
        self.wire
            .subs
            .get(&(key, sub_id.to_string()))
            .map(|s| s.state.clone())
    }

    /// Snapshot of the registered M2 follow-feed `InterestId`s.
    #[cfg(test)]
    pub(crate) fn follow_feed_interest_ids_for_test(&self) -> Vec<crate::planner::InterestId> {
        self.follow_feed_interest_ids.iter().cloned().collect()
    }

    /// Snapshot of the follow-derived `timeline_authors` projection.
    #[cfg(test)]
    pub(crate) fn timeline_authors_for_test(&self) -> &std::collections::BTreeSet<String> {
        &self.timeline_authors
    }

    /// Count of events currently parked in the V-59 pre-kind:3 buffer.
    #[cfg(test)]
    pub(crate) fn pre_kind3_buffer_len_for_test(&self) -> usize {
        self.pre_kind3_buffer.len()
    }

    /// Whether the pre-kind:3 buffer holds an event with `event_id`.
    #[cfg(test)]
    pub(crate) fn pre_kind3_buffer_contains_for_test(&self, event_id: &str) -> bool {
        self.pre_kind3_buffer.contains_key(event_id)
    }

    /// Read-only snapshot of `profile_requests.pending` (the queued kind:0
    /// fetch set).
    #[cfg(test)]
    pub(crate) fn profile_requests_pending_for_test(&self) -> &std::collections::BTreeSet<String> {
        &self.profile_requests.pending
    }

    /// Read-only snapshot of `profile_requests.requested` (the inflight /
    /// completed kind:0 fetch set).
    #[cfg(test)]
    pub(crate) fn profile_requests_requested_for_test(&self) -> &std::collections::HashSet<String> {
        &self.profile_requests.requested
    }
}
