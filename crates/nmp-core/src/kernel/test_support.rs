//! Test-support helpers for the kernel.
//!
//! Test-only injection paths for production ingest hot-paths without real
//! secp256k1 signatures.
//!
use super::*;

mod claim_expansion;
mod preverified_support;

// Claim-expansion test-observation registry (thread-local). Re-exported so the
// existing `test_support::<fn>` call sites in `relay_score_record.rs` and the
// claim-expansion tests keep resolving after the split for the file-size cap.
pub(crate) use self::claim_expansion::{
    get_claim_expansion_author, mark_claim_expansion_match_seen, take_claim_expansion_match_seen,
};
#[cfg(test)]
pub(crate) use self::claim_expansion::{clear_claim_expansion_subs, register_claim_expansion_sub};

pub(crate) fn test_support_now() -> Instant {
    Instant::now()
}

#[cfg(test)]
struct ProfileViewSeedParser {
    lookup: std::sync::Arc<crate::substrate::TestProfileLookup>,
    display: String,
}

#[cfg(test)]
impl crate::substrate::IngestParser for ProfileViewSeedParser {
    fn parse(&self, evt: &crate::store::VerifiedEvent) {
        let raw = evt.raw();
        self.lookup.seed_view(
            &raw.pubkey,
            crate::substrate::ProfileView {
                event_id: raw.id.clone(),
                created_at: raw.created_at,
                display: self.display.clone(),
                ..Default::default()
            },
        );
    }
}

impl Kernel {
    /// Test-support constructor for downstream protocol crates.
    #[must_use]
    pub fn testing_new(visible_limit: usize) -> Self {
        Self::new(visible_limit)
    }

    /// Test-only: register a non-parsing writer that stores an already-owned
    /// [`crate::substrate::ProfileView`] when the dispatcher receives kind:0.
    ///
    /// This exercises the core dispatch/transition seam without duplicating
    /// NIP-01 JSON parsing or cache supersession rules.
    #[cfg(test)]
    pub(crate) fn install_profile_view_seed_parser_for_test(&mut self, display: &str) {
        self.register_ingest_parser(
            0,
            std::sync::Arc::new(ProfileViewSeedParser {
                lookup: std::sync::Arc::clone(&self.test_profile_lookup),
                display: display.to_string(),
            }),
        );
    }

    /// Deliver a replaceable event to the kernel, bypassing signature verification.
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
            .insert(verified.clone(), &relay_url.to_string(), received_at_ms)
        {
            Ok(o) => o,
            Err(_) => return None,
        };
        if matches!(
            outcome,
            InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. }
        ) {
            self.project_accepted_event(&verified);
        }
        Some(outcome)
    }

    /// Seed a fully-formed kind:1 note into the kernel's read-cache (`events`).
    ///
    /// Used by the reaction / thread tests in `actor/commands/tests.rs` to
    /// stage a parent note so a subsequent `react(..., target_id)` resolves
    /// the parent author from the read-cache (`event_author`) rather than the
    /// uncached fallback. Bypasses the store entirely — purely a read-cache
    /// fixture. The `tags` argument can carry whatever NIP-10 structure the
    /// test needs.
    // Test-support helper: `pub(crate)` so in-crate `#[cfg(test)]` modules can call
    // it, but those call sites are invisible to the `test-support` feature build,
    // so the compiler warns without this allow.
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
    // Test-support helper: `pub(crate)` so in-crate `#[cfg(test)]` modules can call
    // it, but those call sites are invisible to the `test-support` feature build,
    // so the compiler warns without this allow.
    #[allow(dead_code)]
    pub(crate) fn seed_kind10002_for_test(&mut self, author_pubkey: &str, write_urls: &[&str]) {
        // Use the author's pubkey as the synthetic event ID — guaranteed
        // unique per author in a fresh-kernel test. The old two-char prefix
        // approach caused a Duplicate hit when the randomly-generated active
        // pubkey started with the same two hex chars as FIATJAF_HEX ("3b")
        // or SEED_NPUB_HEX ("fa"), making the store return Duplicate for that
        // author.
        let id = author_pubkey.to_string();
        let tags: Vec<Vec<String>> = write_urls
            .iter()
            .map(|url| vec!["r".to_string(), url.to_string(), "write".to_string()])
            .collect();
        // Use a far-future `created_at` so the seeded relay list always wins the
        // replaceable-event dedup in `store::insert` (strict `>` on `created_at`).
        // Account creation publishes an onboarding kind:10002 stamped with the
        // kernel clock; a fixed past timestamp would lose that race and the
        // seeded list would be silently discarded. `u64::MAX` guarantees the
        // test seed overrides whatever production state was already accepted.
        let verified = crate::store::VerifiedEvent::from_raw_unchecked(crate::store::RawEvent {
            id,
            pubkey: author_pubkey.to_string(),
            created_at: u64::MAX,
            kind: 10002,
            tags,
            content: String::new(),
            sig: "a".repeat(128),
        });
        let _ = self
            .store
            .insert(verified, &"wss://seed".to_string(), 1_700_000_000_000);
        self.mailbox_cache.upsert(
            author_pubkey.to_string(),
            crate::substrate::ParsedRelayList {
                read: Vec::new(),
                write: write_urls.iter().map(|url| (*url).to_string()).collect(),
                both: Vec::new(),
            },
        );
        self.lifecycle
            .enqueue_trigger(crate::subs::CompileTrigger::Nip65Arrived {
                pubkey: author_pubkey.to_string(),
                created_at: u64::MAX,
            });
    }

    /// Test seam for delivering a kind:3 contact list through the genuine
    /// store-first projection path — NOT a parallel fake writer.
    ///
    /// Reconstructs a `VerifiedEvent` from `event`, inserts it into the event
    /// store, and only then runs [`Kernel::project_accepted_event`] for accepted
    /// inserts/replacements. The active-account contacts-transition signal
    /// derives from the accepted event and enqueues the source recompile trigger
    /// (`on_active_contacts_changed`). This is the SAME ordering a
    /// relay-delivered, locally-published, or cache-served kind:3 takes.
    ///
    /// Test-support only — gated on `cfg(any(test, feature = "test-support"))`.
    // Test-support helper: `pub(in crate::kernel)` so kernel-submodule test code
    // can call it; those call sites are `#[cfg(test)]`-gated and therefore
    // invisible to the `test-support` feature build, so the compiler warns.
    #[allow(dead_code)]
    pub(in crate::kernel) fn inject_contacts(&mut self, event: NostrEvent) {
        let verified = crate::store::VerifiedEvent::from_raw_unchecked(crate::store::RawEvent {
            id: event.id.clone(),
            pubkey: event.pubkey.clone(),
            created_at: event.created_at,
            kind: event.kind,
            tags: event.tags.clone(),
            content: event.content.clone(),
            sig: if event.sig.is_empty() {
                "a".repeat(128)
            } else {
                event.sig.clone()
            },
        });
        let outcome = self.store.insert(
            verified.clone(),
            &"wss://test.invalid/".to_string(),
            event.created_at.saturating_mul(1_000),
        );
        if matches!(
            outcome,
            Ok(crate::store::InsertOutcome::Inserted { .. })
                | Ok(crate::store::InsertOutcome::Replaced { .. })
                | Ok(crate::store::InsertOutcome::Ephemeral { .. })
        ) {
            self.project_accepted_event(&verified);
        }
    }

    /// Lazily install (and return) the test-only
    /// [`crate::substrate::TestDmInboxRelayCache`] behind the kernel's
    /// `dm_inbox_relays` slot. First call installs a fresh cache;
    /// subsequent calls return the same `Arc` so seeds compose.
    ///
    /// Test-support only — production composition installs
    /// `nmp_nip17::DmRelayCache` via
    /// [`Kernel::set_dm_inbox_relay_lookup`] instead.
    // Test-support helper: `pub(crate)` so in-crate `#[cfg(test)]` modules can call
    // it, but those call sites are invisible to the `test-support` feature build,
    // so the compiler warns without this allow.
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
    // Test-support helper: `pub(crate)` so in-crate `#[cfg(test)]` modules can call
    // it, but those call sites are invisible to the `test-support` feature build,
    // so the compiler warns without this allow.
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

    /// Snapshot of the home timeline author projection.
    #[cfg(test)]
    pub(crate) fn timeline_authors_for_test(&self) -> &std::collections::BTreeSet<String> {
        &self.timeline_authors
    }

    /// True iff a kind:0 claim interest is registered for `pubkey` (M2).
    #[cfg(test)]
    pub(crate) fn profile_claim_interest_registered_for_test(&self, pubkey: &str) -> bool {
        self.profile_claim_interest_lifecycle_for_test(pubkey)
            .is_some()
    }

    /// `Some(true)`=Tailing, `Some(false)`=OneShot, `None`=unregistered.
    #[cfg(test)]
    pub(crate) fn profile_claim_interest_lifecycle_for_test(&self, pubkey: &str) -> Option<bool> {
        self.lifecycle
            .registry()
            .iter_active()
            .into_iter()
            .find_map(|i| {
                (i.shape.kinds.len() == 1
                    && i.shape.kinds.contains(&0)
                    && i.shape.authors.contains(pubkey))
                .then(|| matches!(i.lifecycle, crate::planner::InterestLifecycle::Tailing))
            })
    }

    /// Read-only snapshot of the implicit-discovery probed-mailbox set.
    #[cfg(test)]
    pub(crate) fn probed_mailboxes_for_test(&self) -> &std::collections::BTreeSet<String> {
        self.lifecycle.probed_mailboxes()
    }
}
