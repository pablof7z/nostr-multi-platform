//! D2 coverage-gate policy crate.
//!
//! `nmp-core`'s `SubscriptionLifecycle` exposes a `PlanCoverageHook` seam
//! (`crates/nmp-core/src/subs/mod.rs`) that lets the host rewrite a
//! `CompiledPlan` between M2's compile step and the wire-emitter's `plan_diff`.
//! The seam exists so D2 ("negentropy reconciliation before REQ
//! subscriptions") can be enforced without `nmp-core` growing app nouns or
//! coverage-policy vocabulary.
//!
//! This crate ships the **pure policy data** that drives the hook: thresholds
//! and back-off rules. The hook closure itself — the part that actually mutates
//! a `CompiledPlan` — lives in the assembly crate (today `nmp-app-chirp`, in
//! future a generic `nmp-app-base`), which is the only layer entitled to
//! depend on both `nmp-core` and this crate.
//!
//! # Why a separate crate
//!
//! - **No dep cycle.** Any coverage-policy implementation must reference
//!   `CompiledPlan` from `nmp-core`. If the policy lived in `nmp-core`, the
//!   hook itself would too — but the seam exists *because* coverage policy is
//!   above `nmp-core` in the dep graph (D0: kernel never grows app nouns).
//!   Splitting policy data into a sub-`nmp-core` crate lets the assembly
//!   crate sit above both without inducing a cycle.
//! - **Reusable.** A future second app (e.g. a Marmot app or a generic relay
//!   client) reuses the same thresholds without re-deriving them.
//! - **Testable in isolation.** Policy decisions are pure functions of
//!   `usize`/`u64` inputs — no plan, no actor, no I/O.
//!
//! # D2 doctrine
//!
//! For large or potentially stale interest sets (many authors, many
//! historical events), a negentropy set-reconciliation round-trip has lower
//! relay overhead than a blind REQ. The gate decides when the ratio tips:
//! below `author_negentropy_threshold` the negentropy handshake overhead
//! exceeds the bandwidth saved; at or above it, set-reconciliation savings
//! dominate.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// D2 coverage-gate policy: controls when the assembly crate should rewrite a
/// `CompiledPlan` to prefer negentropy/set-reconciliation over raw REQ.
///
/// This crate has **zero `nmp-core` dependency** — it is pure policy data.
/// The assembly crate (e.g. `nmp-app-chirp`) depends on both `nmp-core` and
/// this crate, creates a `PlanCoverageHook` closure that consults these
/// thresholds, and installs it via `SubscriptionLifecycle::set_coverage_hook`.
///
/// # D2 doctrine
///
/// "negentropy reconciliation before REQ subscriptions": for large or
/// potentially stale interest sets (many authors, many historical events),
/// a negentropy set-reconciliation round-trip has lower relay overhead than
/// a blind REQ. The gate decides when the ratio tips.
#[derive(Clone, Debug)]
pub struct CoverageGate {
    /// Minimum number of distinct authors in a compiled plan before the gate
    /// considers negentropy more efficient than a raw REQ.
    ///
    /// Default: 50 authors. At fewer than 50 follows the overhead of a
    /// negentropy handshake exceeds the bandwidth saved; above 50 the
    /// set-reconciliation savings dominate.
    pub author_negentropy_threshold: usize,

    /// `since` bump factor: when negentropy is selected, the assembly crate
    /// may add `floor(watermark_age_seconds * since_bump_factor)` to `since`
    /// as a relay-load back-off (avoids re-fetching events the store already
    /// holds on a cold reconnect).
    ///
    /// Default: `0.05` (5% of the gap since the last stored event). Zero
    /// disables the bump entirely.
    pub since_bump_factor: f64,

    /// Hard cap on the number of relay connections in any single compiled
    /// plan. The assembly crate may use this to prune `per_relay` after the
    /// M2 compiler runs.
    ///
    /// Default: 30 (matches `subs::DEFAULT_SELECT_MAX_CONNECTIONS`).
    pub max_relay_connections: usize,
}

impl Default for CoverageGate {
    fn default() -> Self {
        Self {
            author_negentropy_threshold: 50,
            since_bump_factor: 0.05,
            max_relay_connections: 30,
        }
    }
}

impl CoverageGate {
    /// Returns `true` when the plan's total author count justifies a
    /// negentropy round-trip over a direct REQ.
    ///
    /// The assembly crate calls this with the sum of unique authors across
    /// all `RelayPlan` entries in the compiled plan.
    #[must_use] 
    pub fn should_use_negentropy(&self, total_author_count: usize) -> bool {
        total_author_count >= self.author_negentropy_threshold
    }

    /// Compute a `since` bump (seconds) to add to the existing `since`
    /// watermark, given the age of the most-recent stored event.
    ///
    /// Returns `0` when `since_bump_factor` is zero or the bump rounds down.
    #[must_use] 
    pub fn since_bump_secs(&self, watermark_age_secs: u64) -> u64 {
        (watermark_age_secs as f64 * self.since_bump_factor) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- defaults ---------------------------------------------------------

    /// Pin the published defaults so any change is a deliberate edit caught
    /// by review (these values are quoted in the rustdoc and in V-05's
    /// BACKLOG entry).
    #[test]
    fn default_values_are_pinned() {
        let gate = CoverageGate::default();
        assert_eq!(gate.author_negentropy_threshold, 50);
        assert!((gate.since_bump_factor - 0.05).abs() < f64::EPSILON);
        assert_eq!(gate.max_relay_connections, 30);
    }

    // -- should_use_negentropy --------------------------------------------

    #[test]
    fn should_use_negentropy_false_below_threshold() {
        let gate = CoverageGate::default();
        // threshold = 50; anything strictly below must NOT trigger negentropy.
        assert!(!gate.should_use_negentropy(0));
        assert!(!gate.should_use_negentropy(1));
        assert!(!gate.should_use_negentropy(49));
    }

    #[test]
    fn should_use_negentropy_true_at_and_above_threshold() {
        let gate = CoverageGate::default();
        // boundary value MUST trigger — the rustdoc contract is `>=`, not `>`.
        assert!(gate.should_use_negentropy(50));
        assert!(gate.should_use_negentropy(51));
        assert!(gate.should_use_negentropy(10_000));
    }

    #[test]
    fn should_use_negentropy_respects_custom_threshold() {
        let gate = CoverageGate {
            author_negentropy_threshold: 5,
            ..CoverageGate::default()
        };
        assert!(!gate.should_use_negentropy(4));
        assert!(gate.should_use_negentropy(5));
        assert!(gate.should_use_negentropy(6));
    }

    // -- since_bump_secs --------------------------------------------------

    #[test]
    fn since_bump_secs_default_factor() {
        let gate = CoverageGate::default(); // factor = 0.05
        // 0 age → 0 bump.
        assert_eq!(gate.since_bump_secs(0), 0);
        // 100s * 0.05 = 5s.
        assert_eq!(gate.since_bump_secs(100), 5);
        // 1000s * 0.05 = 50s.
        assert_eq!(gate.since_bump_secs(1000), 50);
        // 3600s * 0.05 = 180s.
        assert_eq!(gate.since_bump_secs(3600), 180);
    }

    #[test]
    fn since_bump_secs_zero_factor_disables_bump() {
        let gate = CoverageGate {
            since_bump_factor: 0.0,
            ..CoverageGate::default()
        };
        assert_eq!(gate.since_bump_secs(0), 0);
        assert_eq!(gate.since_bump_secs(100), 0);
        assert_eq!(gate.since_bump_secs(1_000_000), 0);
    }

    #[test]
    fn since_bump_secs_rounds_down() {
        // factor 0.05 * watermark 19s = 0.95s → floor to 0.
        let gate = CoverageGate::default();
        assert_eq!(gate.since_bump_secs(19), 0);
        // factor 0.05 * watermark 20s = 1.0s → 1.
        assert_eq!(gate.since_bump_secs(20), 1);
    }

    #[test]
    fn since_bump_secs_custom_factor() {
        let gate = CoverageGate {
            since_bump_factor: 0.5,
            ..CoverageGate::default()
        };
        assert_eq!(gate.since_bump_secs(0), 0);
        assert_eq!(gate.since_bump_secs(10), 5);
        assert_eq!(gate.since_bump_secs(101), 50); // floor of 50.5
    }
}
