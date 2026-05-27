//! Relay-author score lookup seam (W4).
//!
//! A substrate-level read-only trait that lets `apply_selection` ask "is
//! this relay warm for this author?" without depending on nmp-core. The
//! production impl lives in `nmp-core/kernel/relay_score_lookup_impl.rs`
//! (`impl RelayAuthorScoreLookup for Kernel`). Tests and wasm paths use
//! [`NoopRelayAuthorScoreLookup`], which always returns 0.0 / false.
//!
//! # Doctrine guards
//!
//! - **D0** — no protocol nouns; keys are `(author: &str, relay: &str)`.
//! - **D3** — used as a *filter* only (drops cold relay/author pairs before
//!   the greedy pass); it never adds a new routing lane.
//! - **D6** — trait methods are total, return owned `f32` / `bool`; no
//!   `Result`, no panic.
//! - **D8** — `is_warm` delegates to `weight` which is an O(log N) BTreeMap
//!   lookup; no allocation per call.
//!
//! # `WARM_THRESHOLD`
//!
//! 0.40 — admits a single-hit cell (weight = 1/(1+0+1) ≈ 0.50) but
//! excludes a hit paired with a miss (1/(1+1+1) ≈ 0.33). See §8.5 Gigi
//! math in `docs/design/relay-search-radius-impl-plan.md`.
//! The kernel-side mirror lives in `kernel/relay_score.rs::WARM_THRESHOLD`;
//! they must stay in sync — a test in `relay_score_lookup_impl_tests.rs`
//! asserts equality to catch drift.

/// Score floor at-or-above which a `(author, relay)` cell is considered
/// "warm" for Phase-1 selection bias. See module doc for the Gigi math.
pub const WARM_THRESHOLD: f32 = 0.40;

/// Read-only relay-author warmth seam.
///
/// The planner calls [`Self::is_warm`] for each `(author, relay)` pair
/// being considered in `apply_selection`. The production implementation
/// (`impl RelayAuthorScoreLookup for Kernel`) consults the kernel's
/// live in-memory `RelayAuthorScoreMap` via `&self` so that A6
/// same-tick visibility holds: claim A's score delta written in the same
/// actor tick is visible to claim B's compile pass.
pub trait RelayAuthorScoreLookup {
    /// Combined `[0.0, 1.0]` weight for `(author, relay)`.
    ///
    /// Unknown pairs (no prior claims) return `0.0`.
    /// The URL is canonicalized internally by the implementation.
    fn weight(&self, author: &str, relay: &str) -> f32;

    /// `true` iff `weight(author, relay) >= WARM_THRESHOLD`.
    ///
    /// Default impl is a one-liner over `weight`; implementations may
    /// override for a single-lookup short-circuit.
    fn is_warm(&self, author: &str, relay: &str) -> bool {
        self.weight(author, relay) >= WARM_THRESHOLD
    }
}

/// No-op fallback — always cold. Default for tests, wasm, and any path
/// where no score store has been injected.
///
/// Using `NoopRelayAuthorScoreLookup` preserves the pre-W4 behaviour:
/// `apply_selection` receives `Some(&NoopRelayAuthorScoreLookup)` in
/// tests and behaves exactly as if no lookup were provided (`None`).
/// The `noop_lookup_preserves_existing_behaviour` test guards this.
pub struct NoopRelayAuthorScoreLookup;

impl RelayAuthorScoreLookup for NoopRelayAuthorScoreLookup {
    fn weight(&self, _author: &str, _relay: &str) -> f32 {
        0.0
    }
}
