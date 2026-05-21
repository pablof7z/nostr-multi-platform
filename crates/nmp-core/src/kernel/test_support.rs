//! Test-support helpers for the kernel.
//!
//! All items in this file are gated on `cfg(any(test, feature = "test-support"))`.
//! They provide fast, signature-verification-free injection paths that let
//! unit tests and the firehose/FFI stress harnesses exercise the same ingest
//! hot-paths as production code without needing real secp256k1 keys.
//!
//! New test-support helpers should be added here rather than to `kernel/mod.rs`
//! to keep the main module under the 300-LOC soft limit (AGENTS.md).
//!
//! D0: capability boundary respected — none of these methods appear in the
//! production FFI surface.

use super::*;

impl Kernel {
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
                10002 => self.ingest_relay_list(event),
                10050 => self.ingest_dm_relay_list(event),
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
    /// Used by the T144 publish-reply tests in `actor/commands/tests.rs` to
    /// stage a parent note so `publish_note(..., Some(parent_id))` exercises
    /// the warm-reply path (`reply_tags_for_parent`) rather than the
    /// cold-reply hydration fallback. Bypasses the store entirely — purely a
    /// read-cache fixture. The `tags` argument can carry whatever NIP-10
    /// structure the test needs to assert root-forwarding on.
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

    /// Read-only check that an id is sitting on the T121 thread-hydration
    /// queue (either pending or already requested). Used by the cold-reply
    /// test to assert the hydration REQ was kicked.
    #[allow(dead_code)]
    pub(crate) fn is_thread_hydration_requested(&self, id: &str) -> bool {
        self.thread_view.requested_ids.contains(id) || self.thread_view.pending_ids.contains(id)
    }

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

    /// Seed a kind:10050 (NIP-17 DM-relay list) into the kernel's event store
    /// and DM-relay-list cache for `author_pubkey` with `dm_relay_urls` as its
    /// `relay`-tag DM-inbox relays.
    ///
    /// The structural analogue of `seed_kind10002_for_test`: tests that exercise
    /// the NIP-17 DM send path's `recipient_dm_relays` resolution call this to
    /// prime a recipient's kind:10050 list before dispatching a DM command.
    /// The synthetic event id is the author pubkey (unique per author in a
    /// fresh-kernel test); `created_at: u64::MAX` guarantees the seed wins the
    /// replaceable-event supersession check in `store::insert`.
    ///
    /// Test-support only — gated on `cfg(any(test, feature = "test-support"))`.
    #[allow(dead_code)]
    pub(crate) fn seed_kind10050_for_test(&mut self, author_pubkey: &str, dm_relay_urls: &[&str]) {
        let id = author_pubkey.to_string();
        let tags: Vec<Vec<String>> = dm_relay_urls
            .iter()
            .map(|url| vec!["relay".to_string(), url.to_string()])
            .collect();
        self.inject_replaceable_event(
            &id,
            author_pubkey,
            u64::MAX,
            10050,
            tags,
            "wss://seed",
            1_700_000_000_000,
        );
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
    pub(crate) fn register_wire_frames_for_test(
        &mut self,
        frames: &[crate::subs::WireFrame],
    ) {
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
    pub(crate) fn follow_feed_interest_ids_for_test(
        &self,
    ) -> Vec<crate::planner::InterestId> {
        self.follow_feed_interest_ids.iter().cloned().collect()
    }

    /// Snapshot of the follow-derived `timeline_authors` projection.
    #[cfg(test)]
    pub(crate) fn timeline_authors_for_test(&self) -> &std::collections::BTreeSet<String> {
        &self.timeline_authors
    }
}

