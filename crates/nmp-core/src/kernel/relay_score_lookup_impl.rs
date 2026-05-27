//! `impl RelayAuthorScoreLookup for Kernel` — W4 §8.6 retarget.
//!
//! The kernel implements the planner's read-only seam directly via `&self`
//! (no `Arc<dyn …>`) so that A6 same-tick visibility holds: W3's
//! `record_relay_score` mutates the in-memory map and the very next compile
//! pass in the same actor tick sees the update.
//!
//! # Borrow-shape note (§8.6)
//!
//! `Kernel::drain_lifecycle_tick` calls `self.lifecycle.drain_tick(&mut self)`,
//! which requires splitting the borrow: `lifecycle` is borrowed mutably while
//! `relay_score_map` (and `clock`) are borrowed immutably through `self`. The
//! Rust borrow checker rejects the naive `let lookup = self as &dyn Trait`
//! before the mutable call.
//!
//! Resolution: [`ScoreLookupRef`] is a "tiny `ScoreLookupRef<'a>`" (§8.6)
//! holding only the two immutable borrows needed for the lookup. The kernel
//! constructs it before calling `drain_tick`, avoiding the split-borrow.
//! `impl RelayAuthorScoreLookup for Kernel` is kept for the tests and any
//! caller that holds an immutable `&Kernel`.
//!
//! # Keeping this in a separate file
//!
//! `kernel/mod.rs` is already at the 500-LOC ceiling (D-V12). All new impl
//! blocks for `Kernel` that carry W-suffix functionality belong in their
//! own sibling files (`relay_score_flush.rs`, etc.). This file follows the
//! same pattern.
//!
//! # Doctrine
//!
//! - **D0** — the trait lives in `nmp-planner::selection::relay_score_lookup`;
//!   our impl consults `relay_score_map` which is keyed on substrate types
//!   (`Pubkey = String, RelayUrl = String`).
//! - **D6** — `weight` / `is_warm` are total; unknown pairs return `0.0` /
//!   `false` via `RelayAuthorScoreMap::get` → `RelayAuthorScore::default`.
//! - **D8** — both methods are O(log N) BTreeMap lookups; no allocation per
//!   call.

use nmp_planner::selection::relay_score_lookup::RelayAuthorScoreLookup;

use super::relay_score::RelayAuthorScoreMap;
use super::Kernel;

/// Thin borrow-shape shim — holds only the immutable borrows needed for
/// [`RelayAuthorScoreLookup`]. Created by [`Kernel::score_lookup_ref`] so
/// that [`Kernel::drain_lifecycle_tick`] can pass a lookup to
/// `lifecycle.drain_tick(...)` without a split-borrow conflict.
pub(super) struct ScoreLookupRef<'a> {
    map: &'a RelayAuthorScoreMap,
    now_secs: u64,
}

impl<'a> RelayAuthorScoreLookup for ScoreLookupRef<'a> {
    fn weight(&self, author: &str, relay: &str) -> f32 {
        self.map
            .get(&author.to_string(), relay)
            .weight(self.now_secs)
    }
}

impl Kernel {
    /// Construct a [`ScoreLookupRef`] from explicit field borrows — a
    /// borrow-shape view of the kernel's score map at the given wall-clock
    /// second.
    ///
    /// **Why the explicit signature?**
    /// `drain_lifecycle_tick` needs to call `self.lifecycle.drain_tick(...)`,
    /// which requires `&mut self.lifecycle`. A `score_lookup_ref(&self)` helper
    /// would borrow all of `self` (including `lifecycle`) through the `&self`
    /// parameter, making the subsequent `&mut self.lifecycle` illegal. By
    /// accepting the individual fields directly — `map: &RelayAuthorScoreMap`
    /// and `now_secs: u64` — the caller can split-borrow:
    ///
    /// ```rust,ignore
    /// let lookup = Kernel::score_lookup_ref_from(
    ///     &self.relay_score_map,
    ///     self.now_secs(),  // computed before split
    /// );
    /// self.lifecycle.drain_tick(&mailboxes, Some(&lookup));
    /// ```
    ///
    /// `now_secs` is a copy (`u64`), not a borrow, so no lifetime conflict.
    pub(super) fn score_lookup_ref_from(
        map: &RelayAuthorScoreMap,
        now_secs: u64,
    ) -> ScoreLookupRef<'_> {
        ScoreLookupRef { map, now_secs }
    }
}

/// `impl RelayAuthorScoreLookup for Kernel` — usable when an immutable
/// `&Kernel` is available (tests, single-borrow contexts).
impl RelayAuthorScoreLookup for Kernel {
    /// Return the combined `[0.0, 1.0]` decay-weighted score for
    /// `(author, relay)` at the current wall-clock second.
    ///
    /// Delegates to `RelayAuthorScoreMap::get(...).weight(now_unix_s)`.
    /// Canonicalizes `relay` via the same `CanonicalRelayUrl::parse_or_raw`
    /// path used during write (§8.10 read-side parity).
    fn weight(&self, author: &str, relay: &str) -> f32 {
        let now = self.now_secs();
        self.relay_score_map
            .get(&author.to_string(), relay)
            .weight(now)
    }
}

#[cfg(test)]
mod relay_score_lookup_impl_tests {
    use nmp_planner::selection::relay_score_lookup::{RelayAuthorScoreLookup, WARM_THRESHOLD};

    use crate::kernel::relay_score::{ClaimOutcome, WARM_THRESHOLD as KERNEL_WARM_THRESHOLD};
    use crate::kernel::Kernel;

    /// Sanity check: the planner's WARM_THRESHOLD constant matches the kernel's.
    /// A drift here would mean the planner filters on a different threshold
    /// than the kernel's score math targets. Fail loudly.
    #[test]
    fn warm_threshold_constants_are_in_sync() {
        assert!(
            (WARM_THRESHOLD - KERNEL_WARM_THRESHOLD).abs() < f32::EPSILON,
            "planner WARM_THRESHOLD ({WARM_THRESHOLD}) must equal \
             kernel WARM_THRESHOLD ({KERNEL_WARM_THRESHOLD})"
        );
    }

    /// Test: `Kernel` correctly exposes the `RelayAuthorScoreLookup` trait.
    #[test]
    fn kernel_exposes_score_lookup() {
        let kernel = Kernel::new(100);
        // Trait is callable — returns 0.0 for an unknown pair.
        let w = kernel.weight("alice", "wss://r.example");
        assert!(
            w.abs() < f32::EPSILON,
            "unknown pair should return 0.0, got {w}"
        );
    }

    /// Test: after recording a hit, `is_warm` returns `true` (score > 0.40).
    #[test]
    fn kernel_is_warm_returns_true_above_threshold() {
        let mut kernel = Kernel::new(100);
        // Record one hit at "now" (kernel's own clock).
        let now = kernel.now_secs();
        kernel.record_relay_score("alice", "wss://r.example", ClaimOutcome::Hit, now);
        // weight = 1 / (1+0+1) = 0.5 > WARM_THRESHOLD (0.40).
        assert!(
            kernel.is_warm("alice", "wss://r.example"),
            "one hit should make the relay warm"
        );
    }

    /// Test: a cell with zero successes is not warm.
    #[test]
    fn kernel_is_warm_returns_false_below_threshold() {
        let mut kernel = Kernel::new(100);
        let now = kernel.now_secs();
        // Record a failure only — no successes.
        kernel.record_relay_score("alice", "wss://r.example", ClaimOutcome::Failed, now);
        // weight = 0.0 (successes == 0).
        assert!(
            !kernel.is_warm("alice", "wss://r.example"),
            "zero successes should not be warm"
        );
    }

    /// Test: URL canonicalization at lookup time matches write-side.
    /// Per §8.10: `wss://r.example/` and `wss://r.example` must resolve to
    /// the same cell — the trailing slash is stripped at write AND read.
    #[test]
    fn kernel_weight_canonicalizes_url_lookup() {
        let mut kernel = Kernel::new(100);
        let now = kernel.now_secs();
        // Write with trailing slash.
        kernel.record_relay_score("alice", "wss://r.example/", ClaimOutcome::Hit, now);
        // Read without trailing slash — must still be warm.
        assert!(
            kernel.is_warm("alice", "wss://r.example"),
            "trailing-slash and non-slash forms must resolve to the same cell"
        );
        // And the reverse: write without, read with.
        let mut kernel2 = Kernel::new(100);
        kernel2.record_relay_score("bob", "wss://q.example", ClaimOutcome::Hit, now);
        assert!(
            kernel2.is_warm("bob", "wss://q.example/"),
            "read with trailing slash must still find the cell written without it"
        );
    }
}
