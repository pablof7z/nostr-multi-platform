//! `CompiledPlan`, `RelayPlan`, `SubShape`, and `RoutingSource` — the output
//! types produced by the subscription compiler.
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.3–§3.4
//! Doctrine: D6 (planner errors are internal Results, never cross FFI).

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use super::interest::{InterestId, InterestShape, Pubkey, RelayUrl};
use crate::stable_hash::stable_hash64;

// ─── UserConfiguredCategory ──────────────────────────────────────────────────

/// Sub-category for `RoutingSource::UserConfigured`.
///
/// Indexer fallback is NOT a fifth diagnostic lane — it is `UserConfigured`
/// with sub-category `Indexer`. This preserves the four-lane discipline
/// (`docs/design/subscription-compilation/diagnostics.md` §5.0 + §5.1 Lane 4)
/// so the diagnostic UI always sees exactly four columns regardless of whether
/// an author is served via NIP-65, hints, provenance, or any user-configured
/// sub-category.
///
/// `ByLaneCounts::indexer_fallback` in the coverage view exposes the indexer
/// sub-count WITHOUT splitting lane 4 — it is a sub-count of `user_configured`,
/// not an extra lane.
#[derive(Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum UserConfiguredCategory {
    /// User's own read relays (overrides NIP-65 read for the active account).
    AccountRead,
    /// User's own write relays.
    AccountWrite,
    /// Kernel-configured indexer relay (e.g. purplepag.es).
    ///
    /// Discovery-only: the indexer set is used to fetch kind:0 / kind:3 /
    /// kind:10002 lookups while NIP-65 mailboxes are being populated. It is
    /// NEVER a content fallback for kind:1 / kind:30023 / etc. — content
    /// REQs only ride NIP-65 write relays (or `AppRelay`). The only way an
    /// indexer URL ends up carrying content is if an author independently
    /// declares it in their own kind:10002 write set, in which case it is
    /// routed by `Nip65`, not by being-the-indexer. Never used for writes (D3).
    Indexer,
    /// Operator-configured app relays. Additive to NIP-65 in both directions;
    /// substitutes when NIP-65 is unknown. Distinct from [`Indexer`] (which is
    /// discovery-only, never content).
    ///
    /// REQ-side semantics:
    /// - Author with NIP-65 mailbox → union of `outbox_relays` AND `app_relays`.
    /// - Author with no NIP-65 mailbox → `app_relays` only (no indexer fallback
    ///   for content). If `app_relays` is also empty, the author lands in
    ///   `CompiledPlan::unroutable_authors` and the kernel surfaces a toast.
    ///
    /// [`Indexer`]: UserConfiguredCategory::Indexer
    AppRelay,
    /// Operator-injected relay for debug/testing purposes.
    Debug,
}

// ─── RoutingSource ───────────────────────────────────────────────────────────

/// Why a relay was included in the plan.
///
/// A relay may appear for multiple reasons simultaneously (e.g., both NIP-65
/// and user-configured). `RelayPlan::role_tags` is a `BTreeSet<RoutingSource>`
/// preserving all reasons — the four-lane diagnostic discipline requires that
/// lanes are never collapsed.
///
/// **Indexer fallback** is represented as `UserConfigured(UserConfiguredCategory::Indexer)`,
/// NOT as a separate variant. There are exactly four lanes in the diagnostic model
/// (NIP-65 / Hint / Provenance / User-configured); the indexer is a sub-category
/// of lane 4. See `docs/design/subscription-compilation/diagnostics.md` §5.0.
///
/// Design: `docs/design/subscription-compilation/diagnostics.md` §5.2
#[derive(Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum RoutingSource {
    /// Resolved from the author's published kind:10002 relay list (lane 1).
    Nip65,
    /// Resolved from a routing hint embedded in an event tag (lane 2).
    Hint,
    /// Observed as the provenance relay for a prior event (lane 3).
    Provenance,
    /// Resolved from a user-configured or operator-policy relay set (lane 4).
    ///
    /// Includes indexer fallback as `UserConfigured(UserConfiguredCategory::Indexer)`.
    /// The sub-category is carried here so that `RelayPlan::role_tags` remains
    /// self-describing without consulting a separate fact stream.
    UserConfigured(UserConfiguredCategory),
}

// ─── SubShape ────────────────────────────────────────────────────────────────

/// A single merged filter that will be emitted as one wire REQ.
///
/// The wire-emitter renders each `SubShape` as exactly one `["REQ", sub_id, filter]`
/// frame. The `canonical_filter_hash` provides stable identity for ADR-0007
/// `WireSubscriptionStatus` records across re-emissions.
///
/// # Wire-emitter lifecycle field
/// Add `lifecycle: InterestLifecycle` to this struct when the wire-emitter lands.
/// The compiler already computes lifecycle during the Stage 3 greedy merge;
/// lifecycle equality is enforced by Rule 6 before any two shapes are merged.
/// The wire-emitter needs lifecycle to decide whether to send CLOSE on EOSE
/// (OneShot) or keep the subscription open (Tailing).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubShape {
    /// The canonical, post-merge filter description.
    pub shape: InterestShape,
    /// All logical interests whose filters were merged into this sub-shape.
    pub originating_interests: Vec<InterestId>,
    /// Canonical hash of the serialised `shape` for stable wire-subscription identity.
    /// Current stop-gap format is 8 hex chars; see [`canonical_filter_hash`].
    pub canonical_filter_hash: String,
}

impl SubShape {
    /// Recompute [`Self::canonical_filter_hash`] from the current `shape`.
    ///
    /// Required by any post-compile pass that mutates the shape (the M4
    /// coverage gate is the only current consumer — it bumps `since` after the
    /// compiler runs). Without this call the wire-emitter's diff would treat
    /// the mutated shape as identical to the pre-mutation one and skip the
    /// REQ frame — leaving the relay on a stale `since`. See
    /// `docs/design/subscription-compilation/compiler.md` §3.3 and the M4
    /// codex review at `docs/perf/codex-reviews/076173d.md` (P1 plan-identity
    /// bug).
    pub fn recompute_hash(&mut self) {
        self.canonical_filter_hash = canonical_filter_hash(&self.shape);
    }
}

/// Canonical filter hash — single source of truth for `(filter, relay)`
/// identity across the planner, wire-emitter, and watermark store.
///
/// The current implementation is the stop-gap stable FNV digest produced by
/// the compiler since M2 (see `compiler/mod.rs::simple_shape_hash`). It is
/// stable across recompiles of an identical `InterestShape` because every
/// collection field uses a sorted container (`BTreeSet` / `BTreeMap`) and the
/// JSON serialisation is therefore deterministic.
///
/// Replacement target — once the BLAKE3-CBOR canonical encoding described in
/// `docs/design/lmdb/watermarks.md` §3 lands, this function swaps to the
/// 32-byte BLAKE3 hex form; the eight-character window will widen accordingly.
/// All callers (compiler, planner gate, wire-emitter, watermark store) read
/// this single helper so the swap is one edit.
pub fn canonical_filter_hash(shape: &InterestShape) -> String {
    let hash = serde_json::to_string(shape)
        .map(|json| stable_hash64(("canonical-filter", json)))
        .unwrap_or_else(|_| stable_hash64("canonical-filter-invalid-json"));
    format!("{:08x}", hash & 0xffff_ffff)
}

// ─── RelayPlan ───────────────────────────────────────────────────────────────

/// The per-relay slice of a `CompiledPlan`: which sub-shapes (REQs) go to
/// this relay and why.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RelayPlan {
    /// The relay this plan slice targets.
    pub relay_url: RelayUrl,
    /// Why this relay is in the plan (may contain multiple sources).
    pub role_tags: BTreeSet<RoutingSource>,
    /// Each sub-shape becomes one wire REQ on this relay.
    pub sub_shapes: Vec<SubShape>,
}

// ─── CompiledPlan ────────────────────────────────────────────────────────────

/// The output of the subscription compiler: a per-relay mapping of what REQs
/// to emit.
///
/// `plan_id` is the stable identity the platform observes for diagnostic
/// continuity. It is content-addressed over the interest set, mailbox snapshot,
/// and lattice version — so two compiles with no material change produce the
/// same id (idempotency check).
///
/// Design: `docs/design/subscription-compilation/compiler.md` §3.4
/// Doctrine: D6 (errors are internal Results), D8 (composite reverse index).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompiledPlan {
    /// Stable, content-addressed plan identity.
    ///
    /// `plan_id = hash(sorted_interests, sorted_mailbox_snapshot, lattice_version)`
    /// (see compiler.md §3.4 for the full hash definition).
    pub plan_id: String,

    /// Per-relay plans, keyed by relay URL for diffing.
    pub per_relay: BTreeMap<RelayUrl, RelayPlan>,

    /// Authors that had neither a NIP-65 mailbox nor an app-relay landing
    /// pad — they produced zero relay entries and the kernel must surface a
    /// diagnostic (e.g. a toast) so the user knows the request will not fly.
    ///
    /// Derived state, NOT part of `plan_id` hashing — adding or removing
    /// app relays at runtime must not invalidate a plan's identity for the
    /// wire-emitter's diff. The kernel reads this set to drive UI signal,
    /// not the wire-emitter.
    #[serde(default)]
    pub unroutable_authors: BTreeSet<Pubkey>,
}

impl CompiledPlan {
    /// Returns an empty plan with the given id (used by tests and stubs).
    pub fn empty(plan_id: impl Into<String>) -> Self {
        Self {
            plan_id: plan_id.into(),
            per_relay: BTreeMap::new(),
            unroutable_authors: BTreeSet::new(),
        }
    }
}

// ─── PlannerError ────────────────────────────────────────────────────────────

/// Internal planner error type.
///
/// Per D6, this type NEVER crosses the FFI boundary. The actor-boundary
/// mapping is wired (`SubscriptionLifecycle::drain_tick` records genuine
/// errors into `last_planner_error`; `Kernel::make_update` projects that
/// recorded string into the `KernelUpdate`/FFI envelope — #171).
///
/// #171 status: these variants are presently DEFENSIVE-ONLY. The sole
/// compiler path, `compile_with_context`, always returns `Ok` (an empty
/// interest set yields an empty plan, not `EmptyInterestSet`; no shape
/// validation or hashing-failure path constructs `InvalidShape` /
/// `HashingFailed` today). The enum is kept so the `Result` API stays closed
/// and the projection wiring above means any future genuine construction
/// path surfaces through the FFI with no further D6 work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlannerError {
    /// No interests were registered; nothing to compile.
    EmptyInterestSet,
    /// An interest's shape is internally inconsistent (e.g. `until < since`).
    InvalidShape { reason: String },
    /// Serialisation of the interest set for plan-id hashing failed.
    HashingFailed { reason: String },
}

impl std::fmt::Display for PlannerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyInterestSet => write!(f, "no interests registered"),
            Self::InvalidShape { reason } => write!(f, "invalid shape: {reason}"),
            Self::HashingFailed { reason } => write!(f, "plan-id hashing failed: {reason}"),
        }
    }
}

impl std::error::Error for PlannerError {}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::interest::InterestId;

    /// Deterministic 64-char hex pubkey / event-id fixture from a single byte.
    fn hex(byte: &str) -> String {
        byte.repeat(32)
    }

    // ── canonical_filter_hash ────────────────────────────────────────────────

    #[test]
    fn canonical_filter_hash_is_deterministic_for_identical_shapes() {
        // Two shapes built independently from the same field values must hash
        // identically — the §3.4 plan-id stability contract depends on this.
        let a = InterestShape::timeline_for([hex("aa"), hex("bb")].into_iter().collect());
        let b = InterestShape::timeline_for([hex("bb"), hex("aa")].into_iter().collect());
        // Same logical shape (BTreeSet sorts insertion order away).
        assert_eq!(a, b);
        // ...therefore the same canonical hash.
        assert_eq!(canonical_filter_hash(&a), canonical_filter_hash(&b));
        // And re-hashing the very same value is idempotent.
        assert_eq!(canonical_filter_hash(&a), canonical_filter_hash(&a));
    }

    #[test]
    fn canonical_filter_hash_differs_for_distinct_shapes() {
        // Clearly distinct author sets → distinct hashes. Collision risk for
        // wholly different inputs is negligible for a 32-bit window.
        let aa = InterestShape::timeline_for([hex("aa")].into_iter().collect());
        let bb = InterestShape::timeline_for([hex("bb")].into_iter().collect());
        assert_ne!(canonical_filter_hash(&aa), canonical_filter_hash(&bb));

        // A different kind set on the same authors also changes the hash.
        let profile = InterestShape::profile_for(hex("aa"));
        assert_ne!(canonical_filter_hash(&aa), canonical_filter_hash(&profile));
    }

    #[test]
    fn canonical_filter_hash_handles_empty_shape_without_panicking() {
        // The all-wildcard default shape must hash cleanly (no panic, no empty
        // string) — the compiler emits an empty plan for an empty interest set.
        let empty = InterestShape::default();
        let hash = canonical_filter_hash(&empty);
        assert!(!hash.is_empty());
        // The empty shape is stable across calls just like any other.
        assert_eq!(hash, canonical_filter_hash(&InterestShape::default()));
    }

    #[test]
    fn canonical_filter_hash_emits_eight_hex_chars() {
        // Documented stop-gap format: an 8-char lowercase hex string. Every
        // caller (wire-emitter diff, watermark store) relies on this width.
        for shape in [
            InterestShape::default(),
            InterestShape::timeline_for([hex("aa")].into_iter().collect()),
            InterestShape::profile_for(hex("cc")),
        ] {
            let hash = canonical_filter_hash(&shape);
            assert_eq!(hash.len(), 8, "hash must be 8 chars: {hash}");
            assert!(
                hash.chars().all(|c| c.is_ascii_hexdigit()),
                "hash must be hex: {hash}"
            );
        }
    }

    // ── SubShape::recompute_hash ─────────────────────────────────────────────

    #[test]
    fn recompute_hash_refreshes_a_stale_hash_from_the_current_shape() {
        // Start with a SubShape carrying a deliberately wrong cached hash.
        let mut sub = SubShape {
            shape: InterestShape::timeline_for([hex("aa")].into_iter().collect()),
            originating_interests: vec![InterestId(1)],
            canonical_filter_hash: "deadbeef".to_string(),
        };
        let expected_before = canonical_filter_hash(&sub.shape);

        // Mutate the shape (the M4 coverage gate bumps `since` post-compile).
        sub.shape.since = Some(1_700_000_000);
        let expected_after = canonical_filter_hash(&sub.shape);
        // The mutation genuinely changed the canonical hash.
        assert_ne!(expected_before, expected_after);

        // recompute_hash must adopt the post-mutation shape's hash, discarding
        // both the stale "deadbeef" and the pre-mutation value.
        sub.recompute_hash();
        assert_eq!(sub.canonical_filter_hash, expected_after);
        assert_ne!(sub.canonical_filter_hash, "deadbeef");
    }

    #[test]
    fn recompute_hash_ignores_originating_interests() {
        // Only `shape` is hashed — the originating-interest provenance list is
        // not part of wire identity. Two SubShapes with identical shapes but
        // different originating interests must produce the same hash.
        let shape = InterestShape::timeline_for([hex("aa")].into_iter().collect());
        let mut one = SubShape {
            shape: shape.clone(),
            originating_interests: vec![InterestId(1)],
            canonical_filter_hash: String::new(),
        };
        let mut many = SubShape {
            shape,
            originating_interests: vec![InterestId(7), InterestId(9), InterestId(42)],
            canonical_filter_hash: String::new(),
        };
        one.recompute_hash();
        many.recompute_hash();
        assert_eq!(one.canonical_filter_hash, many.canonical_filter_hash);
    }

    // ── CompiledPlan::empty ──────────────────────────────────────────────────

    #[test]
    fn compiled_plan_empty_carries_the_given_id_and_no_relay_plans() {
        let plan = CompiledPlan::empty("plan-abc123");
        // The supplied id is carried verbatim.
        assert_eq!(plan.plan_id, "plan-abc123");
        // No relay plans and no unroutable authors — a truly empty plan.
        assert!(plan.per_relay.is_empty());
        assert!(plan.unroutable_authors.is_empty());

        // `impl Into<String>` accepts an owned String too.
        let owned = CompiledPlan::empty(String::from("owned-id"));
        assert_eq!(owned.plan_id, "owned-id");
    }
}
