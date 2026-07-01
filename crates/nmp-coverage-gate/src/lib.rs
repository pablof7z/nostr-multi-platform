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
//! a `CompiledPlan` — lives in an assembly crate (e.g. the external Chirp
//! repo's `nmp-app-chirp`, or a future generic `nmp-app-base`), which is the
//! only layer entitled to depend on both `nmp-core` and this crate.
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
//! For large or potentially stale interest sets, a negentropy
//! set-reconciliation round-trip has lower relay overhead than a blind REQ.
//! The gate decides when the ratio tips by looking at the filter's possible
//! result surface. Older callers still express that surface as author × kind
//! fanout; newer NIP-77 callers classify the full NIP-01 filter directly.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// D2 coverage-gate policy: controls when the assembly crate should rewrite a
/// `CompiledPlan` to prefer negentropy/set-reconciliation over raw REQ.
///
/// This crate has **zero `nmp-core` dependency** — it is pure policy data.
/// The assembly crate (e.g. the external Chirp repo's `nmp-app-chirp`)
/// depends on both `nmp-core` and this crate, creates a `PlanCoverageHook`
/// closure that consults these thresholds, and installs it via
/// `SubscriptionLifecycle::set_coverage_hook`.
///
/// # D2 doctrine
///
/// "negentropy reconciliation before REQ subscriptions": for large or
/// potentially stale interest sets (many authors, many historical events),
/// a negentropy set-reconciliation round-trip has lower relay overhead than
/// a blind REQ. The gate decides when the ratio tips.
#[derive(Clone, Debug)]
pub struct CoverageGate {
    /// Minimum author × kind fanout for a single filter before the gate
    /// considers negentropy more efficient than a raw REQ. Also used as the
    /// known-result upper-bound threshold for full NIP-01 filters.
    ///
    /// Default: 50 author-kind pairs. At smaller fanouts the overhead of a
    /// negentropy handshake usually exceeds the bandwidth saved; at or above
    /// 50 the set-reconciliation savings dominate. The count is per filter,
    /// not per plan, so `3 kinds × 20 authors = 60` qualifies.
    pub filter_fanout_negentropy_threshold: usize,

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
            filter_fanout_negentropy_threshold: 50,
            since_bump_factor: 0.05,
            max_relay_connections: 30,
        }
    }
}

impl CoverageGate {
    /// Returns `true` when a single filter's author × kind fanout justifies a
    /// negentropy round-trip over a direct REQ and the target relay is known to
    /// support negentropy.
    ///
    /// Empty authors or empty kinds return `false`: those filters are not the
    /// large author-list fanout this policy is designed to steer.
    #[must_use]
    pub fn should_use_negentropy_for_filter(
        &self,
        fanout: FilterFanout,
        relay_supports_negentropy: bool,
    ) -> bool {
        self.should_use_negentropy_for_result_surface(
            ResultSurface::KnownMax(fanout.author_kind_pairs()),
            relay_supports_negentropy,
        )
    }

    /// Returns `true` when a full NIP-01 filter has a large or unbounded possible
    /// result surface and the relay can support negentropy.
    ///
    /// Known-small surfaces, such as a handful of exact ids or an exact
    /// addressable key, should use a plain REQ. Unknown/unbounded surfaces, such
    /// as `#e` threads or regular kind history, are good NIP-77 candidates.
    #[must_use]
    pub fn should_use_negentropy_for_result_surface(
        &self,
        surface: ResultSurface,
        relay_supports_negentropy: bool,
    ) -> bool {
        relay_supports_negentropy
            && match surface {
                ResultSurface::KnownMax(max) => max >= self.filter_fanout_negentropy_threshold,
                ResultSurface::Unbounded => true,
            }
    }

    /// Compute a `since` bump (seconds) to add to the existing `since`
    /// watermark, given the age of the most-recent stored event.
    ///
    /// Returns `0` when `since_bump_factor` is zero or the bump rounds down.
    #[must_use]
    pub fn since_bump_secs(&self, watermark_age_secs: u64) -> u64 {
        // watermark_age_secs is a cache age in seconds — practically bounded
        // by a few years (well within f64's 2^53 exact-integer range).
        // since_bump_factor is always >= 0.0 per construction, so sign loss is
        // impossible. The .max(0.0) guard makes both invariants explicit.
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        let bump = (watermark_age_secs as f64 * self.since_bump_factor).max(0.0) as u64;
        bump
    }
}

/// The author × kind surface area of a single Nostr filter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FilterFanout {
    author_count: usize,
    kind_count: usize,
}

/// The statically-known upper bound for a NIP-01 filter's possible result set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResultSurface {
    /// The filter can return at most this many events.
    KnownMax(usize),
    /// The filter has no static finite bound from NIP-01 semantics.
    Unbounded,
}

impl FilterFanout {
    /// Construct a filter fanout from already-normalized filter dimensions.
    #[must_use]
    pub const fn new(author_count: usize, kind_count: usize) -> Self {
        Self {
            author_count,
            kind_count,
        }
    }

    /// Number of authors in the filter.
    #[must_use]
    pub const fn author_count(self) -> usize {
        self.author_count
    }

    /// Number of explicit kinds in the filter.
    #[must_use]
    pub const fn kind_count(self) -> usize {
        self.kind_count
    }

    /// Author × kind product used by the D2 negentropy threshold.
    #[must_use]
    pub const fn author_kind_pairs(self) -> usize {
        if self.author_count == 0 || self.kind_count == 0 {
            0
        } else {
            self.author_count.saturating_mul(self.kind_count)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- defaults ---------------------------------------------------------

    /// Pin the published defaults so any change is a deliberate edit caught
    /// by review (these values are quoted in the rustdoc and in V-05's
    /// tracker entry).
    #[test]
    fn default_values_are_pinned() {
        let gate = CoverageGate::default();
        assert_eq!(gate.filter_fanout_negentropy_threshold, 50);
        assert!((gate.since_bump_factor - 0.05).abs() < f64::EPSILON);
        assert_eq!(gate.max_relay_connections, 30);
    }

    // -- should_use_negentropy_for_filter ---------------------------------

    #[test]
    fn should_use_negentropy_for_filter_false_below_threshold() {
        let gate = CoverageGate::default();
        assert!(!gate.should_use_negentropy_for_filter(FilterFanout::new(0, 1), true));
        assert!(!gate.should_use_negentropy_for_filter(FilterFanout::new(1, 0), true));
        assert!(!gate.should_use_negentropy_for_filter(FilterFanout::new(49, 1), true));
        assert!(!gate.should_use_negentropy_for_filter(FilterFanout::new(24, 2), true));
    }

    #[test]
    fn should_use_negentropy_for_filter_true_at_and_above_threshold() {
        let gate = CoverageGate::default();
        assert!(gate.should_use_negentropy_for_filter(FilterFanout::new(50, 1), true));
        assert!(gate.should_use_negentropy_for_filter(FilterFanout::new(25, 2), true));
        assert!(gate.should_use_negentropy_for_filter(FilterFanout::new(20, 3), true));
        assert!(gate.should_use_negentropy_for_filter(FilterFanout::new(10_000, 1), true));
    }

    #[test]
    fn should_use_negentropy_for_filter_requires_relay_support() {
        let gate = CoverageGate::default();
        assert!(
            !gate.should_use_negentropy_for_filter(FilterFanout::new(25, 2), false),
            "large filters must still use raw REQ when NIP-77 support is not known"
        );
    }

    #[test]
    fn should_use_negentropy_for_filter_respects_custom_threshold() {
        let gate = CoverageGate {
            filter_fanout_negentropy_threshold: 5,
            ..CoverageGate::default()
        };
        assert!(!gate.should_use_negentropy_for_filter(FilterFanout::new(2, 2), true));
        assert!(gate.should_use_negentropy_for_filter(FilterFanout::new(5, 1), true));
        assert!(gate.should_use_negentropy_for_filter(FilterFanout::new(2, 3), true));
    }

    #[test]
    fn should_use_negentropy_for_result_surface_uses_known_max() {
        let gate = CoverageGate::default();
        assert!(
            !gate.should_use_negentropy_for_result_surface(ResultSurface::KnownMax(3), true),
            "small exact surfaces should use plain REQ"
        );
        assert!(
            gate.should_use_negentropy_for_result_surface(ResultSurface::KnownMax(50), true),
            "known surfaces at the threshold should use NIP-77"
        );
    }

    #[test]
    fn should_use_negentropy_for_result_surface_accepts_unbounded() {
        let gate = CoverageGate::default();
        assert!(gate.should_use_negentropy_for_result_surface(ResultSurface::Unbounded, true));
        assert!(!gate.should_use_negentropy_for_result_surface(ResultSurface::Unbounded, false));
    }

    #[test]
    fn filter_fanout_product_is_saturating() {
        let fanout = FilterFanout::new(usize::MAX, 2);
        assert_eq!(fanout.author_count(), usize::MAX);
        assert_eq!(fanout.kind_count(), 2);
        assert_eq!(fanout.author_kind_pairs(), usize::MAX);
    }

    // -- since_bump_secs --------------------------------------------------

    #[test]
    fn since_bump_secs_default_factor() {
        // Default factor = 0.05.
        let gate = CoverageGate::default();
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

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
