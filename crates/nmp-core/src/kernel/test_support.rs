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

mod preverified_support;

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
                // ADR-0057 PR 2 — kind:0 flows through the GENUINE post-store
                // projection path: reconstruct the `VerifiedEvent` (the store
                // above already accepted it) and run the shared
                // `project_accepted_event`, which dispatches to the registered
                // kind:0 parser (`TestKind0Parser` in test builds, the real
                // `nmp_nip01::Kind0Parser` in production) → writes the profile
                // cache → bumps `profiles_ver`. No parallel fake writer.
                0 => {
                    let verified = VerifiedEvent::from_raw_unchecked(RawEvent {
                        id: event.id.clone(),
                        pubkey: event.pubkey.clone(),
                        created_at: event.created_at,
                        kind: event.kind,
                        tags: event.tags.clone(),
                        content: event.content.clone(),
                        sig: "a".repeat(128),
                    });
                    self.project_accepted_event(&verified);
                }
                // ADR-0057 PR 3 — kind:3 flows through the GENUINE post-store
                // projection path (like kind:0 above): reconstruct the
                // `VerifiedEvent` (the store above already accepted it) and run
                // the shared `project_accepted_event`, which dispatches to the
                // registered kind:3 parser (`TestKind3Parser` in test builds,
                // the real `nmp_nip01::Kind3Parser` in production) → writes the
                // contacts cache → the active-account contacts-transition signal
                // drives the kernel-owned follow-feed effects. No parallel fake
                // writer.
                3 => {
                    let verified = VerifiedEvent::from_raw_unchecked(RawEvent {
                        id: event.id.clone(),
                        pubkey: event.pubkey.clone(),
                        created_at: event.created_at,
                        kind: event.kind,
                        tags: event.tags.clone(),
                        content: event.content.clone(),
                        sig: "a".repeat(128),
                    });
                    self.project_accepted_event(&verified);
                }
                10002 => {
                    // The production kind:10002 writer is the substrate
                    // `nmp_router::Kind10002Parser`, which this helper bypasses
                    // (it skips `verify_and_persist`/the dispatcher). Substitute
                    // its effect inline: parse `r` tags into a `ParsedRelayList`,
                    // upsert (or remove on empty) into the substrate `MailboxCache`,
                    // and enqueue the `Nip65Arrived` recompile trigger — exactly
                    // what `Kernel::on_mailbox_changed` does in production.
                    let parsed =
                        parse_relay_list_to_substrate(&event.tags);
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
                    // M2: the `Nip65Arrived` trigger enqueued above is the whole
                    // re-route mechanism now (the next recompile routes the
                    // registered kind:0 claim onto the author's new write relays);
                    // `refresh_profile_after_mailbox` is deleted.
                    let _ = mailbox_mutated;
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

    /// Test seam for delivering a kind:0 profile through the GENUINE post-store
    /// projection path (ADR-0057 PR 2) — NOT a parallel fake writer.
    ///
    /// Reconstructs a `VerifiedEvent` from `event` and runs the shared
    /// [`Kernel::project_accepted_event`], which dispatches to the REGISTERED
    /// kind:0 parser (`TestKind0Parser` in test builds, `nmp_nip01::Kind0Parser`
    /// in production) → the parser writes the capability-owned profile cache →
    /// the transition sweep bumps `profiles_ver`. This is the SAME path a relay-delivered
    /// or cache-served kind:0 takes; there is no separate cache writer. Callers
    /// keep the convenient `NostrEvent`-taking signature.
    ///
    /// Test-support only — gated on `cfg(any(test, feature = "test-support"))`.
    #[allow(dead_code)]
    pub(in crate::kernel) fn inject_profile(&mut self, event: NostrEvent) {
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
        self.project_accepted_event(&verified);
    }

    /// Test seam for delivering a kind:3 contact list through the GENUINE
    /// post-store projection path (ADR-0057 PR 3) — NOT a parallel fake writer.
    ///
    /// Reconstructs a `VerifiedEvent` from `event` and runs the shared
    /// [`Kernel::project_accepted_event`], which dispatches to the REGISTERED
    /// kind:3 parser (`TestKind3Parser` in test builds, `nmp_nip01::Kind3Parser`
    /// in production) → the parser writes the capability-owned contacts cache →
    /// the contacts-transition signal for the ACTIVE account drives the
    /// kernel-owned follow-feed effects (`on_active_contacts_changed`:
    /// `FollowListChanged` trigger + `sync_follow_feed_interests` →
    /// `timeline_authors` rebuild + cache-serve). This is the SAME path a
    /// relay-delivered, locally-published, or cache-served kind:3 takes; there
    /// is no separate cache writer. Callers keep the convenient
    /// `NostrEvent`-taking signature.
    ///
    /// Test-support only — gated on `cfg(any(test, feature = "test-support"))`.
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
        self.project_accepted_event(&verified);
    }

    /// Test seam: install a FRESH empty `TestContactsCache` behind the kernel's
    /// `contacts_lookup` slot — the in-crate equivalent of a cold restart losing
    /// the in-memory contacts cache (which production rebuilds from the store via
    /// cache-serve). Used by the ADR-0057 PR 3 cold-restart cache-serve test.
    #[cfg(test)]
    pub(crate) fn clear_test_contacts_cache(&mut self) {
        // Clear the CONTENTS in place (not swap the `Arc`) so the registered
        // `TestKind3Parser` (which holds the same `Arc`) keeps writing the cache
        // the kernel's `contacts_lookup` reader reads — the parser→cache→reader
        // identity that the contacts-transition detection depends on.
        self.test_contacts_cache.clear();
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

    /// The author set carried by the SINGLE collapsed follow-feed interest
    /// (#1497) — `None` when no follow-feed interest is registered. Looks the
    /// interest up by its tracked id so the test sees exactly the shape that
    /// was installed in the registry.
    #[cfg(test)]
    pub(crate) fn follow_feed_interest_authors_for_test(
        &self,
    ) -> Option<std::collections::BTreeSet<String>> {
        let id = self.follow_feed_interest_ids.iter().next()?;
        self.lifecycle
            .registry()
            .iter_active()
            .into_iter()
            .find(|i| &i.id == id)
            .map(|i| i.shape.authors.clone())
    }

    /// Snapshot of the follow-derived `timeline_authors` projection.
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
