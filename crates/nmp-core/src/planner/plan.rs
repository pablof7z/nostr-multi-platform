//! `CompiledPlan`, `RelayPlan`, `SubShape`, and `RoutingSource` — the output
//! types produced by the subscription compiler.
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.3–§3.4
//! Doctrine: D6 (planner errors are internal Results, never cross FFI).

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::interest::{InterestId, InterestShape, Pubkey, RelayUrl};

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
/// (OneShot / BoundedTime) or keep the subscription open (Tailing).
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
/// The current implementation is the stop-gap `DefaultHasher` digest produced
/// by the compiler since M2 (see `compiler/mod.rs::simple_shape_hash`). It is
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
    let mut h = DefaultHasher::new();
    if let Ok(json) = serde_json::to_string(shape) {
        json.hash(&mut h);
    }
    format!("{:08x}", h.finish() & 0xffff_ffff)
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
/// Per D6, this type NEVER crosses the FFI boundary. Callers at the actor
/// boundary must map `PlannerError` to an observable state update (e.g. a
/// toast string) before it reaches the FFI surface.
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
