//! Score-update seam — translates accepted claim-expansion outcomes
//! (matching EVENT = Hit, EOSE-without-match = EoseNoMatch, relay_failed =
//! Failed) into score deltas via `relay_score::ClaimOutcome`.
//!
//! [`Kernel::record_claim_outcome`] is the single, typed entry point.
//! It converts the kernel's injected wall-clock to a `now_unix_s: u64`,
//! delegates to [`relay_score::RelayAuthorScoreMap::record`] (which
//! applies the scoring contract and sets the dirty flag), and emits a
//! `WireLogEvent::ScoreUpdate` when `NMP_CLAIM_LOG` is set.

use super::{
    relay_score::{ClaimOutcome, RelayAuthorScore},
    wire_log::{log_wire, WireLogEvent},
    Kernel, NostrEvent,
};

impl Kernel {
    /// Record a relay-author score outcome from the claim-lifecycle layer.
    ///
    /// Called by the ingest EVENT/EOSE arms and the `relay_failed` hook (W5)
    /// when a relay delivers (Hit), EOSEs without a match (EoseNoMatch), or
    /// fails (Failed). The `now_unix_s` timestamp is read from the kernel's
    /// injected clock via `self.now_secs()`.
    ///
    /// Routes through [`Kernel::record_relay_score`] / [`Kernel::get_relay_score`]
    /// (D4 accessor seam, §8.3) and emits a `WireLogEvent::ScoreUpdate` diagnostic
    /// line when `NMP_CLAIM_LOG` is set.
    ///
    /// D6: unknown `(author, relay_url)` cells are created on first record.
    /// D4: `&mut self` — the kernel is the sole writer of the score map.
    pub(crate) fn record_claim_outcome(
        &mut self,
        author: &str,
        relay_url: &str,
        outcome: ClaimOutcome,
    ) {
        let now = self.now_secs();
        // D4: route through the accessor seam — `record_relay_score` is the
        // sole write entry point for the score map (§8.3).
        self.record_relay_score(author, relay_url, outcome, now);
        // Emit structured diagnostic line (no-op unless NMP_CLAIM_LOG is set).
        let cell: RelayAuthorScore = self.get_relay_score(author, relay_url);
        let delta = match outcome {
            ClaimOutcome::Hit => "+1s",
            ClaimOutcome::EoseNoMatch => "0",
            ClaimOutcome::Failed => "+3f",
        };
        log_wire(WireLogEvent::ScoreUpdate {
            author,
            relay_url,
            delta,
            new_weight: cell.weight(now),
        });
    }

    /// Return the claim author only when this EVENT belongs to the registered
    /// claim-expansion sub and the event author matches that claim.
    ///
    /// W5 will replace the author-only test seam with the full pending-claim
    /// shape, including event id / address constraints. W3 still performs the
    /// available match check here so invalid or wrong-author relay frames cannot
    /// teach the score map.
    pub(in crate::kernel) fn claim_expansion_match_author(
        &self,
        sub_id: &str,
        event: &NostrEvent,
    ) -> Option<String> {
        let author = self.lookup_claim_expansion_author(sub_id)?;
        (event.pubkey == author).then_some(author)
    }

    /// Record an accepted matching EVENT for a claim-expansion sub.
    ///
    /// W8b: emits `WireLogEvent::EventRx` (NMP_CLAIM_LOG gate) before
    /// recording the score so the telemetry line captures the actual
    /// `event_id`.
    ///
    /// B1: after recording the score, calls `on_claim_outcome_hit(sub_id)`
    /// so the W5 controller transitions the claim to Terminal(Hit) and
    /// claims never linger in pending_claims after a matching EVENT.
    pub(in crate::kernel) fn record_claim_expansion_hit(
        &mut self,
        sub_id: &str,
        relay_url: &str,
        author: &str,
        event_id: &str,
    ) {
        // W8b: structured telemetry for the claim hit path.
        log_wire(WireLogEvent::EventRx {
            sub_id,
            relay_url,
            event_id,
            author,
        });
        self.record_claim_outcome(author, relay_url, ClaimOutcome::Hit);
        self.mark_claim_expansion_match_seen(sub_id, relay_url);
        // B1: drive the W5 controller state machine (production wire-up).
        // on_claim_outcome_hit terminates the claim and cleans up claim_sub_index.
        self.on_claim_outcome_hit(sub_id);
    }

    /// Record EOSE-without-match only if no accepted matching EVENT was seen
    /// for the same `(sub_id, relay_url)` subscription.
    ///
    /// B1: after recording the score, calls `on_claim_outcome_eose_no_match`
    /// so the W5 controller updates in_flight_attempts and advances Phase 2.
    pub(in crate::kernel) fn record_claim_expansion_eose_no_match(
        &mut self,
        sub_id: &str,
        relay_url: &str,
    ) {
        if self.take_claim_expansion_match_seen(sub_id, relay_url) {
            // W8b: EOSE arrived after a matching EVENT was already accepted —
            // emit EoseRx{matched:true} so W9 can distinguish hit-then-eose
            // from no-match-eose in the telemetry stream.
            log_wire(WireLogEvent::EoseRx {
                sub_id,
                relay_url,
                matched: true,
            });
            return;
        }
        if let Some(author) = self.lookup_claim_expansion_author(sub_id) {
            self.record_claim_outcome(&author, relay_url, ClaimOutcome::EoseNoMatch);
        }
        // B1: drive the W5 controller state machine (production wire-up).
        // on_claim_outcome_eose_no_match removes the in_flight_attempt entry
        // and records the relay as attempted.
        self.on_claim_outcome_eose_no_match(sub_id, relay_url);
    }

    /// Returns the author pubkey for a claim-expansion subscription, if any.
    ///
    /// W5 implementation: looks up the originating claim via the twin BTreeMaps
    /// (`claim_sub_index` → `pending_claims` → `author`). Falls back to the
    /// test-support seam for W3 tests that inject via `register_claim_expansion_sub`.
    pub(crate) fn lookup_claim_expansion_author(&self, sub_id: &str) -> Option<String> {
        // Primary path: W5 twin-map lookup (O(log N)).
        if let Some(author) = self.lookup_claim_author_by_sub_id(sub_id) {
            return Some(author);
        }
        // Fallback: test-support seam for W3 tests that inject subs directly.
        self.claim_expansion_sub_author_test(sub_id)
    }

    /// Internal: test-seam lookup, always returns `None` in production.
    fn claim_expansion_sub_author_test(&self, sub_id: &str) -> Option<String> {
        #[cfg(any(test, feature = "test-support"))]
        {
            use super::test_support;
            return test_support::get_claim_expansion_author(sub_id);
        }
        #[cfg(not(any(test, feature = "test-support")))]
        {
            let _ = sub_id;
            None
        }
    }

    fn mark_claim_expansion_match_seen(&self, sub_id: &str, relay_url: &str) {
        #[cfg(any(test, feature = "test-support"))]
        {
            use super::test_support;
            test_support::mark_claim_expansion_match_seen(sub_id, relay_url);
        }
        #[cfg(not(any(test, feature = "test-support")))]
        {
            let _ = (sub_id, relay_url);
        }
    }

    fn take_claim_expansion_match_seen(&self, sub_id: &str, relay_url: &str) -> bool {
        #[cfg(any(test, feature = "test-support"))]
        {
            use super::test_support;
            return test_support::take_claim_expansion_match_seen(sub_id, relay_url);
        }
        #[cfg(not(any(test, feature = "test-support")))]
        {
            let _ = (sub_id, relay_url);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{relay_score::ClaimOutcome, wire_log::write_wire_line, Kernel, NostrEvent};
    use crate::relay::DEFAULT_VISIBLE_LIMIT;

    #[test]
    fn hit_increments_successes_and_sets_now() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        kernel.record_claim_outcome("alice", "wss://r.test", ClaimOutcome::Hit);

        let cell = kernel.get_relay_score("alice", "wss://r.test");
        assert_eq!(cell.successes, 1, "Hit must increment successes");
        assert_eq!(cell.failures, 0, "Hit must not touch failures");
        assert!(cell.last_used_unix_s > 0, "Hit must stamp last_used_unix_s");
    }

    #[test]
    fn eose_no_match_is_neutral_no_score_change() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        // Seed one hit so the cell has a non-zero baseline.
        kernel.record_claim_outcome("alice", "wss://r.test", ClaimOutcome::Hit);
        let cell_after_hit = kernel.get_relay_score("alice", "wss://r.test");

        kernel.record_claim_outcome("alice", "wss://r.test", ClaimOutcome::EoseNoMatch);
        let cell_after_eose = kernel.get_relay_score("alice", "wss://r.test");

        assert_eq!(
            cell_after_hit.successes, cell_after_eose.successes,
            "EoseNoMatch must not change successes"
        );
        assert_eq!(
            cell_after_hit.failures, cell_after_eose.failures,
            "EoseNoMatch must not change failures"
        );
    }

    #[test]
    fn failed_after_retries_increments_failures_by_three() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        kernel.record_claim_outcome("alice", "wss://r.test", ClaimOutcome::Failed);

        let cell = kernel.get_relay_score("alice", "wss://r.test");
        assert_eq!(cell.successes, 0, "Failed must not touch successes");
        assert_eq!(cell.failures, 3, "Failed must add 3 to failures");
    }

    #[test]
    fn dirty_flag_set_after_any_record() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        assert!(
            !kernel.test_relay_score_dirty(),
            "fresh kernel must be clean"
        );

        kernel.record_claim_outcome("alice", "wss://r.test", ClaimOutcome::Hit);
        assert!(
            kernel.test_relay_score_dirty(),
            "Hit must set dirty flag for W2 flush"
        );
    }

    #[test]
    fn record_canonicalizes_url_before_keying() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

        kernel.record_claim_outcome("alice", "wss://r.test/", ClaimOutcome::Hit);
        // Lookup under the alternate spelling.
        let cell = kernel.get_relay_score("alice", "wss://r.test");
        assert_eq!(
            cell.successes, 1,
            "trailing-slash URL must map to the same cell as no-slash URL (§8.10)"
        );
    }

    #[test]
    fn record_emits_score_update_wire_log_event() {
        use super::super::relay_score::RelayAuthorScore;
        use super::super::wire_log::WireLogEvent;

        const NOW: u64 = 1_767_225_600;

        let mut cell = RelayAuthorScore::default();
        cell.record_hit(NOW);
        let event = WireLogEvent::ScoreUpdate {
            author: "alice",
            relay_url: "wss://r.test",
            delta: "+1s",
            new_weight: cell.weight(NOW),
        };

        let mut buf: Vec<u8> = Vec::new();
        write_wire_line(&mut buf, true, &event);

        let output = String::from_utf8(buf).expect("valid UTF-8");
        assert!(
            output.contains("ScoreUpdate"),
            "output must contain 'ScoreUpdate' discriminant; got: {output:?}"
        );
        assert!(
            output.contains("alice"),
            "output must contain author; got: {output:?}"
        );
        assert!(
            output.contains("+1s"),
            "output must contain delta; got: {output:?}"
        );
    }

    #[test]
    fn claim_expansion_event_hit_records_score_after_acceptance() {
        use super::super::test_support;
        use crate::relay::RelayRole;

        test_support::clear_claim_expansion_subs();
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let keys = ::nostr::Keys::generate();

        let sub_id = "claim-exp-test-sub-001";
        let relay_url = "wss://relay.claim-expansion.test";
        let event = signed_note(&keys, "claim hit", 1_700_000_000);
        let author_hex = event.pubkey.clone();

        test_support::register_claim_expansion_sub(sub_id, &author_hex);

        let cell_before = kernel.get_relay_score(&author_hex, relay_url);
        assert_eq!(
            cell_before.successes, 0,
            "cell must start with zero successes"
        );

        kernel.handle_text(RelayRole::Indexer, relay_url, &event_frame(sub_id, &event));

        let cell_after = kernel.get_relay_score(&author_hex, relay_url);
        assert_eq!(
            cell_after.successes, 1,
            "accepted matching EVENT on a claim-expansion sub must record a Hit"
        );

        test_support::clear_claim_expansion_subs();
    }

    #[test]
    fn claim_expansion_invalid_event_does_not_record_hit() {
        use super::super::test_support;
        use crate::relay::RelayRole;

        test_support::clear_claim_expansion_subs();
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let keys = ::nostr::Keys::generate();

        let sub_id = "claim-exp-test-sub-invalid";
        let relay_url = "wss://relay.claim-expansion.test";
        let mut event = signed_note(&keys, "bad signature", 1_700_000_001);
        event.sig = "c".repeat(128);
        let author_hex = event.pubkey.clone();
        test_support::register_claim_expansion_sub(sub_id, &author_hex);

        kernel.handle_text(RelayRole::Indexer, relay_url, &event_frame(sub_id, &event));

        let cell_after = kernel.get_relay_score(&author_hex, relay_url);
        assert_eq!(cell_after.successes, 0);
        assert!(
            !kernel.test_relay_score_dirty(),
            "invalid EVENT must not dirty the relay-score map"
        );

        test_support::clear_claim_expansion_subs();
    }

    #[test]
    fn claim_expansion_wrong_author_event_does_not_record_hit() {
        use super::super::test_support;
        use crate::relay::RelayRole;

        test_support::clear_claim_expansion_subs();
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let claim_keys = ::nostr::Keys::generate();
        let event_keys = ::nostr::Keys::generate();

        let sub_id = "claim-exp-test-sub-wrong-author";
        let relay_url = "wss://relay.claim-expansion.test";
        let claim_author = claim_keys.public_key().to_hex();
        let event = signed_note(&event_keys, "wrong author", 1_700_000_002);
        test_support::register_claim_expansion_sub(sub_id, &claim_author);

        kernel.handle_text(RelayRole::Indexer, relay_url, &event_frame(sub_id, &event));

        let cell_after = kernel.get_relay_score(&claim_author, relay_url);
        assert_eq!(cell_after.successes, 0);
        assert!(
            !kernel.test_relay_score_dirty(),
            "wrong-author EVENT must not dirty the relay-score map"
        );

        test_support::clear_claim_expansion_subs();
    }

    #[test]
    fn claim_expansion_eose_after_hit_does_not_record_no_match() {
        use super::super::test_support;
        use crate::relay::RelayRole;

        test_support::clear_claim_expansion_subs();
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let keys = ::nostr::Keys::generate();

        let sub_id = "claim-exp-test-sub-hit-then-eose";
        let relay_url = "wss://relay.claim-expansion.test";
        let event = signed_note(&keys, "hit before eose", 1_700_000_003);
        let author_hex = event.pubkey.clone();
        test_support::register_claim_expansion_sub(sub_id, &author_hex);

        kernel.handle_text(RelayRole::Indexer, relay_url, &event_frame(sub_id, &event));
        let cell_after_hit = kernel.get_relay_score(&author_hex, relay_url);
        assert_eq!(cell_after_hit.successes, 1);
        kernel.relay_score_map.mark_clean();

        kernel.handle_text(RelayRole::Indexer, relay_url, &eose_frame(sub_id));

        let cell_after_eose = kernel.get_relay_score(&author_hex, relay_url);
        assert_eq!(cell_after_eose.successes, 1);
        assert!(
            !kernel.test_relay_score_dirty(),
            "EOSE after an accepted match must not record EoseNoMatch"
        );

        test_support::clear_claim_expansion_subs();
    }

    #[test]
    fn claim_expansion_eose_without_match_records_neutral_outcome() {
        use super::super::test_support;
        use crate::relay::RelayRole;

        test_support::clear_claim_expansion_subs();
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

        let sub_id = "claim-exp-test-sub-eose-only";
        let relay_url = "wss://relay.claim-expansion.test";
        let author_hex = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        test_support::register_claim_expansion_sub(sub_id, author_hex);
        kernel.record_claim_outcome(author_hex, relay_url, ClaimOutcome::Hit);
        kernel.relay_score_map.mark_clean();

        kernel.handle_text(RelayRole::Indexer, relay_url, &eose_frame(sub_id));

        let cell_after_eose = kernel.get_relay_score(author_hex, relay_url);
        assert_eq!(cell_after_eose.successes, 1);
        assert_eq!(cell_after_eose.failures, 0);
        assert!(
            kernel.test_relay_score_dirty(),
            "EOSE without a matching EVENT must record the neutral recency outcome"
        );

        test_support::clear_claim_expansion_subs();
    }

    fn signed_note(keys: &::nostr::Keys, content: &str, ts: u64) -> NostrEvent {
        use nostr::{EventBuilder, Timestamp};
        let nostr_event = EventBuilder::text_note(content)
            .custom_created_at(Timestamp::from(ts))
            .sign_with_keys(keys)
            .expect("sign_with_keys cannot fail with a generated keypair");
        NostrEvent {
            id: nostr_event.id.to_hex(),
            pubkey: nostr_event.pubkey.to_hex(),
            created_at: nostr_event.created_at.as_secs(),
            kind: nostr_event.kind.as_u16() as u32,
            tags: nostr_event
                .tags
                .iter()
                .map(|t: &::nostr::Tag| t.as_slice().to_vec())
                .collect(),
            content: nostr_event.content.clone(),
            sig: nostr_event.sig.to_string(),
        }
    }

    fn event_frame(sub_id: &str, event: &NostrEvent) -> String {
        serde_json::json!([
            "EVENT",
            sub_id,
            {
                "id": event.id,
                "pubkey": event.pubkey,
                "created_at": event.created_at,
                "kind": event.kind,
                "tags": event.tags,
                "content": event.content,
                "sig": event.sig,
            }
        ])
        .to_string()
    }

    fn eose_frame(sub_id: &str) -> String {
        serde_json::json!(["EOSE", sub_id]).to_string()
    }
}
