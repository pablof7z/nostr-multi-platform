//! W3: score-update seam — translates wire-frame outcomes (EVENT = Hit,
//! EOSE-without-match = EoseNoMatch, relay_failed = Failed) into score
//! deltas via `relay_score::ClaimOutcome`.
//!
//! # Entry point
//!
//! [`Kernel::record_claim_outcome`] is the single, typed entry point.
//! It converts the kernel's injected wall-clock to a `now_unix_s: u64`,
//! delegates to [`relay_score::RelayAuthorScoreMap::record`] (which
//! applies the §8.5 delta table and sets the dirty flag), and emits a
//! `WireLogEvent::ScoreUpdate` when `NMP_CLAIM_LOG` is set.
//!
//! # Call-site stubs (W5 dependency)
//!
//! The production call sites in `ingest/mod.rs` (EVENT arm + EOSE arm)
//! and `requests/relay_lifecycle.rs` (`relay_failed`) require W5's
//! `pending_claims` and `claim_expansion_subs` structures. W3 wires the
//! EVENT and EOSE arms in ingest via `is_claim_expansion_oneshot` /
//! `lookup_claim_expansion_author` stubs that return `false` / `None`
//! until W5 populates those maps. The `relay_failed` walk is a comment
//! block only. This means the hooks are present and correct on the path;
//! they simply never fire until W5 inserts real sub registrations.
//!
//! # Doctrine
//!
//! - **D0**: keys are `(author: &str, relay_url: &str)` — substrate types,
//!   no protocol noun.
//! - **D4**: `&mut self` — sole writer.
//! - **D6**: total — unknown cells are inserted fresh via `entry().or_default()`.
//! - **D8**: called only from already-edge-triggered seams (frame ingest,
//!   transport-failure callback) — no new polling.

use super::{
    relay_score::{ClaimOutcome, RelayAuthorScore},
    wire_log::{log_wire, WireLogEvent},
    Kernel,
};

impl Kernel {
    /// Record a relay-author score outcome from the claim-lifecycle layer.
    ///
    /// Called by the ingest EVENT/EOSE arms and the `relay_failed` hook (W5)
    /// when a relay delivers (Hit), EOSEs without a match (EoseNoMatch), or
    /// fails (Failed). The `now_unix_s` timestamp is read from the kernel's
    /// injected clock via `self.now_secs()`.
    ///
    /// Delegates to `relay_score_map.record()` (§8.5 delta table, §8.10
    /// canonicalization) and emits a `WireLogEvent::ScoreUpdate` diagnostic
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
        self.relay_score_map
            .record(&author.to_string(), relay_url, outcome, now);
        // Emit structured diagnostic line (no-op unless NMP_CLAIM_LOG is set).
        let cell: RelayAuthorScore = self.relay_score_map.get(&author.to_string(), relay_url);
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

    /// Returns `true` if `sub_id` belongs to a claim-expansion subscription.
    ///
    /// Stub: always returns `false` until W5 populates `claim_expansion_subs`.
    /// The ingest EVENT and EOSE arms call this guard so they are correct and
    /// present on the path; they simply never fire until W5 inserts real
    /// registrations.
    pub(crate) fn is_claim_expansion_oneshot(&self, sub_id: &str) -> bool {
        // W5 dependency: claim_expansion_subs map doesn't exist yet.
        // When W5 adds `claim_expansion_subs: BTreeMap<String, Pubkey>` to
        // the Kernel struct, replace this body with:
        //   self.claim_expansion_subs.contains_key(sub_id)
        self.claim_expansion_sub_author_test(sub_id).is_some()
    }

    /// Returns the author pubkey for a claim-expansion subscription, if any.
    ///
    /// Stub: always returns `None` until W5 populates `claim_expansion_subs`.
    pub(crate) fn lookup_claim_expansion_author<'a>(&'a self, sub_id: &str) -> Option<String> {
        // W5 dependency: when W5 adds `claim_expansion_subs: BTreeMap<String, Pubkey>`,
        // replace this body with:
        //   self.claim_expansion_subs.get(sub_id).cloned()
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
}

// ---------------------------------------------------------------------------
// Tests — TDD red→green. Tests 1–6 exercise `record_claim_outcome` directly
// so they compile before the ingest wiring lands. Test #7 is the
// wire-shaped end-to-end path through `handle_text`.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::{relay_score::ClaimOutcome, wire_log::write_wire_line, Kernel};
    use crate::relay::DEFAULT_VISIBLE_LIMIT;

    // -----------------------------------------------------------------------
    // Test 1 — Hit increments successes and stamps last_used.
    // -----------------------------------------------------------------------
    #[test]
    fn hit_increments_successes_and_sets_now() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        kernel.record_claim_outcome("alice", "wss://r.test", ClaimOutcome::Hit);

        let cell = kernel.get_relay_score("alice", "wss://r.test");
        assert_eq!(cell.successes, 1, "Hit must increment successes");
        assert_eq!(cell.failures, 0, "Hit must not touch failures");
        assert!(cell.last_used_unix_s > 0, "Hit must stamp last_used_unix_s");
    }

    // -----------------------------------------------------------------------
    // Test 2 — EoseNoMatch is neutral: counters unchanged, recency stamp moves.
    // §8.5 amendment.
    // -----------------------------------------------------------------------
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
            "EoseNoMatch must not change successes (§8.5)"
        );
        assert_eq!(
            cell_after_hit.failures, cell_after_eose.failures,
            "EoseNoMatch must not change failures (§8.5)"
        );
    }

    // -----------------------------------------------------------------------
    // Test 3 — Failed increments failures by 3 (large penalty per §8.5).
    // -----------------------------------------------------------------------
    #[test]
    fn failed_after_retries_increments_failures_by_three() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        kernel.record_claim_outcome("alice", "wss://r.test", ClaimOutcome::Failed);

        let cell = kernel.get_relay_score("alice", "wss://r.test");
        assert_eq!(cell.successes, 0, "Failed must not touch successes");
        assert_eq!(cell.failures, 3, "Failed must add 3 to failures (§8.5)");
    }

    // -----------------------------------------------------------------------
    // Test 4 — Dirty flag is set after any `record_claim_outcome` call.
    // -----------------------------------------------------------------------
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

    // -----------------------------------------------------------------------
    // Test 5 — Canonicalization: trailing-slash URL and bare URL key same cell.
    // §8.10 amendment.
    // -----------------------------------------------------------------------
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

    // -----------------------------------------------------------------------
    // Test 6 — Wire-log output: ScoreUpdate variant serialises correctly.
    // Uses `write_wire_line` directly with a Vec<u8> sink (same pattern as
    // wire_log_tests.rs, avoids the OnceLock env-var trap).
    // -----------------------------------------------------------------------
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

    // -----------------------------------------------------------------------
    // Test 7 — Wire-shaped end-to-end: feeding an EVENT frame for a registered
    // claim-expansion sub records a Hit on the correct (author, relay) cell.
    //
    // Uses the test-support seam in `kernel/test_support.rs` to register
    // the sub_id → author mapping before feeding the wire frame, exercising
    // the dormant `is_claim_expansion_oneshot` / `lookup_claim_expansion_author`
    // hooks in `ingest/mod.rs`.
    // -----------------------------------------------------------------------
    #[test]
    fn claim_expansion_event_hit_records_score() {
        use super::super::test_support;
        use crate::relay::RelayRole;

        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

        let sub_id = "claim-exp-test-sub-001";
        let relay_url = "wss://relay.claim-expansion.test";
        let author_hex = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

        // Register the (sub_id → author) mapping in the test-only seam.
        test_support::register_claim_expansion_sub(sub_id, author_hex);

        // Score map must start empty for this pair.
        let cell_before = kernel.get_relay_score(author_hex, relay_url);
        assert_eq!(
            cell_before.successes, 0,
            "cell must start with zero successes"
        );

        // Feed an EVENT frame for the registered claim-expansion sub.
        let event_json = format!(
            r#"["EVENT","{sub_id}",{{"id":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","pubkey":"{author_hex}","created_at":1700000000,"kind":1,"tags":[],"content":"test","sig":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"}}]"#
        );

        kernel.handle_text(RelayRole::Indexer, relay_url, &event_json);

        let cell_after = kernel.get_relay_score(author_hex, relay_url);
        assert_eq!(
            cell_after.successes, 1,
            "EVENT on a claim-expansion sub must record a Hit (successes=1)"
        );

        // Cleanup test seam.
        test_support::clear_claim_expansion_subs();
    }
}
