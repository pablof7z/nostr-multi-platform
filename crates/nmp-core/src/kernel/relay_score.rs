//! `RelayAuthorScore` substrate value type + per-author/relay scoring map +
//! pure helpers. The map is the single source of truth queried by the planner
//! warm-relay filter and claim expansion. LMDB hydration happens at kernel
//! construction; dirty cells are flushed on actor idle.
//!
//! Doctrine
//! --------
//! - **D0**: keys are `(Pubkey, RelayUrl)` — substrate types from
//!   `crates/nmp-planner/src/interest.rs`. No protocol noun.
//! - **D4**: every mutation is `&mut self` on the actor-owned `Kernel`.
//!   The reads (`weight`, `is_warm`) take `&self`.
//! - **D6**: every helper is total. Unknown cells return `weight = 0.0`.
//!   No `Result`, no panic. Saturating adds.
//! - **D8**: insertion is `BTreeMap::insert` (O(log N)) only on
//!   edge-triggered seams (EVENT, EOSE-no-match — neutral, Failed).
//!
//! Per the accepted relay-search-radius scoring contract, `EoseNoMatch` is
//! **neutral** (touches `last_used_unix_s` for recency only; the score counters
//! are unchanged). The original "decrement on EoseNoMatch" design demerited
//! good-but-narrow relays out of the warm pool (Gigi math:
//! 10 hits / 40 niche EoseNoMatches → weight ≈ 0.196 < WARM_THRESHOLD).

use std::collections::BTreeMap;

use crate::planner::{Pubkey, RelayUrl};
use crate::relay::CanonicalRelayUrl;

/// Score floor at-or-above which a `(author, relay)` cell is "warm" and
/// eligible for Phase-1 selection bias (W4). 0.40 admits a one-hit cell
/// (`weight = 1/(1+0+1) = 0.50`) but excludes a hit paired with a
/// hypothetical miss; in practice the threshold's only job is to gate the
/// `successes == 0` cold start.
pub const WARM_THRESHOLD: f32 = 0.40;

/// Decay half-life in days. The score's exponential-decay multiplier
/// halves every `DECAY_HALFLIFE_DAYS` of inactivity.
pub const DECAY_HALFLIFE_DAYS: f32 = 14.0;

/// Cap on total relays tried per claim (Phase 1 + Phase 2 union). Above
/// this, the claim terminates `Exhausted`. Spec §6.
#[allow(dead_code)] // consumed by claim_expansion.rs
pub const MAX_RELAYS_TRIED_PER_CLAIM: usize = 12;

/// Max concurrent Phase-2 candidate REQs per claim. Spec §6.
#[allow(dead_code)] // consumed by claim_expansion.rs
pub const MAX_EXPANSION_CONCURRENCY: usize = 3;

/// Wall-clock budget before Phase 1 → Phase 2 transition. Spec §6.
#[allow(dead_code)] // consumed by claim_expansion.rs
pub const PHASE_1_BUDGET_MS: u64 = 1500;

/// Wall-clock budget for any single per-relay REQ in Phase 1 or Phase 2.
/// Beyond this the REQ is considered failed for scoring purposes
/// regardless of whether the socket eventually replies.
#[allow(dead_code)] // consumed by claim_expansion.rs
pub const PER_RELAY_REQ_TIMEOUT_MS: u64 = 5000;

/// User-visible wall-clock budget. After this elapses the tracked claim
/// terminates `Budget`.
#[allow(dead_code)] // consumed by claim_expansion.rs
pub const PER_CLAIM_TOTAL_BUDGET_MS: u64 = 8000;

/// Per-`(Pubkey, RelayUrl)` score cell. Restart-stable: serializes to a
/// 24-byte fixed-width record in LMDB (`u32 + u32 + u64 + u64 reserved`)
/// per §8.9.
///
/// `last_used_unix_s` is the wall-clock UNIX seconds at the last
/// `record_*` call. Decay is computed against this stamp; an old cell
/// fades exponentially.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RelayAuthorScore {
    pub successes: u32,
    pub failures: u32,
    pub last_used_unix_s: u64,
}

impl RelayAuthorScore {
    /// Combined `[0.0, 1.0]` weight under §0 Q1 scheme A:
    ///
    /// `weight = successes / (successes + failures + 1) * exp(-k * age_days)`
    ///
    /// where `k = ln(2) / DECAY_HALFLIFE_DAYS`. The `+1` in the
    /// denominator keeps cold cells from saturating to 1.0 on a single
    /// hit (`1/2 = 0.50`, comfortably above the `0.40` threshold).
    ///
    /// Total: an empty cell (`successes=0, failures=0`) returns `0.0`;
    /// `now < last_used_unix_s` is treated as `age_days = 0` (no
    /// time-travel weighting).
    #[must_use]
    pub fn weight(&self, now_unix_s: u64) -> f32 {
        if self.successes == 0 {
            return 0.0;
        }
        let denom = (self.successes as f32) + (self.failures as f32) + 1.0;
        let raw = (self.successes as f32) / denom;

        let age_seconds = now_unix_s.saturating_sub(self.last_used_unix_s) as f32;
        let age_days = age_seconds / 86_400.0_f32;
        // k = ln(2) / 14 ≈ 0.0495
        let k = std::f32::consts::LN_2 / DECAY_HALFLIFE_DAYS;
        let decay = (-k * age_days).exp();

        raw * decay
    }

    /// `true` iff `weight(now) >= WARM_THRESHOLD`.
    #[must_use]
    pub fn is_warm(&self, now_unix_s: u64) -> bool {
        self.weight(now_unix_s) >= WARM_THRESHOLD
    }

    /// Record a Phase-1 or Phase-2 EVENT match. Saturating-add prevents
    /// `u32::MAX` panic.
    pub fn record_hit(&mut self, now_unix_s: u64) {
        self.successes = self.successes.saturating_add(1);
        self.last_used_unix_s = now_unix_s;
    }

    /// Record an EOSE-without-match. Per the scoring contract this is
    /// **neutral** —
    /// recency stamp moves but counters are unchanged.
    pub fn record_eose_no_match(&mut self, now_unix_s: u64) {
        self.last_used_unix_s = now_unix_s;
    }

    /// Record a transport-side failure (the relay's socket failed
    /// mid-claim). Large decrement (`+3`) reflects high confidence the
    /// relay is unhealthy. Saturating-add.
    pub fn record_failure(&mut self, now_unix_s: u64) {
        self.failures = self.failures.saturating_add(3);
        self.last_used_unix_s = now_unix_s;
    }
}

/// Canonicalize a relay URL with the same idiom used elsewhere in the
/// kernel (`ingest/mod.rs:144` precedent). Scoring under
/// `wss://r.example/` and reading under `wss://r.example` must hit the
/// same cell.
#[inline]
#[must_use]
pub fn canon(relay_url: &str) -> String {
    CanonicalRelayUrl::parse_or_raw(relay_url).into_string()
}

/// In-memory score map. Keyed on `(Pubkey, canonical RelayUrl)`. The
/// single source of truth at runtime; dirty cells persist to LMDB on idle.
#[derive(Debug, Default)]
pub struct RelayAuthorScoreMap {
    cells: BTreeMap<(Pubkey, RelayUrl), RelayAuthorScore>,
    /// `true` if at least one cell mutated since the last LMDB flush.
    /// The flush path clears this; `record_*` calls set it.
    dirty: bool,
}

impl RelayAuthorScoreMap {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up the score cell, canonicalizing `relay_url` first.
    /// Unknown cells return a zero-cell (D6: total). The returned value
    /// is a `Copy`; the map is not borrowed past the call.
    #[must_use]
    pub fn get(&self, author: &Pubkey, relay_url: &str) -> RelayAuthorScore {
        let key = (author.clone(), canon(relay_url));
        self.cells.get(&key).copied().unwrap_or_default()
    }

    /// `true` iff the (canonicalized) cell is warm at `now`.
    #[must_use]
    pub fn is_warm(&self, author: &Pubkey, relay_url: &str, now_unix_s: u64) -> bool {
        self.get(author, relay_url).is_warm(now_unix_s)
    }

    /// Record an outcome for `(author, relay_url)` at `now`. Mutates the
    /// cell in place (creating it if absent) and sets `dirty`.
    pub fn record(
        &mut self,
        author: &Pubkey,
        relay_url: &str,
        outcome: ClaimOutcome,
        now_unix_s: u64,
    ) {
        match outcome {
            ClaimOutcome::EoseNoMatch => {
                let key = (author.clone(), canon(relay_url));
                let Some(cell) = self.cells.get_mut(&key) else {
                    return;
                };
                cell.record_eose_no_match(now_unix_s);
            }
            ClaimOutcome::Hit => {
                let key = (author.clone(), canon(relay_url));
                let cell = self.cells.entry(key).or_default();
                cell.record_hit(now_unix_s);
            }
            ClaimOutcome::Failed => {
                let key = (author.clone(), canon(relay_url));
                let cell = self.cells.entry(key).or_default();
                cell.record_failure(now_unix_s);
            }
        }
        self.dirty = true;
    }

    /// Snapshot of all cells for LMDB flush. Returns owned `Vec`
    /// (D6: no borrow leaks into the store thread).
    #[must_use]
    pub fn snapshot(&self) -> Vec<(Pubkey, RelayUrl, RelayAuthorScore)> {
        self.cells
            .iter()
            .map(|((pk, url), s)| (pk.clone(), url.clone(), *s))
            .collect()
    }

    /// Reset the dirty flag after a successful LMDB flush.
    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Inject cells from LMDB at kernel construct. The store has already
    /// canonicalized URLs on write, so we trust the key shape.
    pub fn bulk_load<I>(&mut self, cells: I)
    where
        I: IntoIterator<Item = (Pubkey, RelayUrl, RelayAuthorScore)>,
    {
        for (pk, url, s) in cells {
            self.cells.insert((pk, url), s);
        }
        // Loading from store is not a "dirty" event (the store already
        // has these rows).
        self.dirty = false;
    }

    /// Diagnostic: total cell count (tests).
    #[cfg(test)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.cells.len()
    }
}

/// Discrete outcome of a per-relay claim observation, fed into
/// `RelayAuthorScoreMap::record`. Mirrors the relay-search-radius scoring
/// contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)] // consumed by relay score recording and claim expansion
pub enum ClaimOutcome {
    /// Phase-1 or Phase-2 EVENT match — successes += 1.
    Hit,
    /// Phase-1 or Phase-2 EOSE without a match — neutral; touches recency.
    EoseNoMatch,
    /// Transport-side failure (`relay_failed`) — failures += 3.
    Failed,
}
