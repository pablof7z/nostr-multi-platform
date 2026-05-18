//! `CompiledPlan`, `RelayPlan`, `SubShape`, and `RoutingSource` — the output
//! types produced by the subscription compiler.
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.3–§3.4
//! Doctrine: D6 (planner errors are internal Results, never cross FFI).

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use super::interest::{InterestId, InterestShape, RelayUrl};

// ─── RoutingSource ───────────────────────────────────────────────────────────

/// Why a relay was included in the plan.
///
/// A relay may appear for multiple reasons simultaneously (e.g., both NIP-65
/// and user-configured). `RelayPlan::role_tags` is a `BTreeSet<RoutingSource>`
/// preserving all reasons — the four-lane diagnostic discipline requires that
/// lanes are never collapsed.
///
/// Design: `docs/design/subscription-compilation/diagnostics.md` §5.2
#[derive(Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum RoutingSource {
    /// Resolved from the author's published kind:10002 relay list.
    Nip65,
    /// Resolved from a user-configured relay set.
    UserConfigured,
    /// Resolved from indexer fallback (no mailbox known for the author).
    Indexer,
    /// Resolved from a routing hint embedded in an event tag.
    Hint,
    /// Observed as the provenance relay for a prior event.
    Provenance,
}

// ─── SubShape ────────────────────────────────────────────────────────────────

/// A single merged filter that will be emitted as one wire REQ.
///
/// The wire-emitter renders each `SubShape` as exactly one `["REQ", sub_id, filter]`
/// frame. The `canonical_filter_hash` provides stable identity for ADR-0007
/// `WireSubscriptionStatus` records across re-emissions.
///
/// # TODO(wire-emitter)
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
    /// Blake3 hash of the serialised `shape` for stable wire-subscription identity.
    /// Placeholder: populated by the compiler stage 4. Format: 8 hex chars.
    pub canonical_filter_hash: String,
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
}

impl CompiledPlan {
    /// Returns an empty plan with the given id (used by tests and stubs).
    pub fn empty(plan_id: impl Into<String>) -> Self {
        Self {
            plan_id: plan_id.into(),
            per_relay: BTreeMap::new(),
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
