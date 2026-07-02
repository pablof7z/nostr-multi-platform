//! `CompiledPlan`, `RelayPlan`, `SubShape`, and `RoutingSource` — the output
//! types produced by the subscription compiler.
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.3–§3.4
//! Doctrine: D6 (planner errors are internal Results, never cross FFI).

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use super::interest::{EventId, InterestId, InterestShape, Pubkey, RelayUrl};
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
    /// Cold-start bootstrap content relay (PD-033-C planner-extension lane).
    ///
    /// Distinct from [`Indexer`] (which is discovery-only, never content) and
    /// from [`AppRelay`] (operator-configured, may be empty before any user
    /// configuration). The kernel populates `bootstrap_content_relays` from
    /// `Kernel::bootstrap_urls_for_role(RelayRole::Content)` — the same
    /// well-known seed (`FALLBACK_CONTENT_RELAY`) it uses for cold-start
    /// connections — so a `OneShot + Global` discovery interest with concrete
    /// `event_ids` always has a landing pad even before any account is loaded.
    ///
    /// Only consumed by Case D for `OneShot + Global + event_ids`-shaped
    /// interests. Cases A/B/C never route to this lane (an `authors`-shape
    /// without a NIP-65 mailbox routes via [`Indexer`] in the same
    /// PD-033-C-bootstrap arm; see Case A's `if !landed` block).
    ///
    /// [`Indexer`]: UserConfiguredCategory::Indexer
    /// [`AppRelay`]: UserConfiguredCategory::AppRelay
    Bootstrap,
    /// A relay the client already holds an active *pinned* subscription to
    /// (the `relay_pin` Case-E lane), reused as a kind:0 resolution landing pad.
    ///
    /// Motivation: in a single-relay NIP-29 group the host relay serves the
    /// members' kind:0 metadata but advertises no NIP-65 (kind:10002), and the
    /// authors may have published a relay list nowhere the app reaches. The
    /// kind:0 is sitting on the relay we are already connected to, yet neither
    /// the outbox lane ([`Nip65`]) nor [`AppRelay`] / [`Indexer`] would route a
    /// profile claim there. This sub-category routes a profile (kind:0-only)
    /// claim additively onto the union of relays the active interest set pins —
    /// "also ask the relay I'm already talking to."
    ///
    /// Strictly scoped: consumed ONLY by Case A for the exact kind:0
    /// profile-resolution shape (`kinds == {0}`). General content interests
    /// (kind:1 timelines, etc.) never fan out to pinned relays, so a follow
    /// author's notes are never leaked to a group relay. It is additive — NIP-65
    /// outbox and [`AppRelay`] still apply; this is one more landing pad, not a
    /// replacement. Never used for writes (D3).
    ///
    /// [`Nip65`]: RoutingSource::Nip65
    /// [`Indexer`]: UserConfiguredCategory::Indexer
    /// [`AppRelay`]: UserConfiguredCategory::AppRelay
    ActivePin,
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
    /// Resolved from the recipient's NIP-17 kind:10050 DM-relay list.
    ///
    /// This is intentionally not folded into [`RoutingSource::Nip65`]:
    /// kind:10050 is a separate private-message relay list, and diagnostics
    /// must not make a gift-wrap inbox look like it rode the public mailbox.
    Nip17DmRelay,
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

// ─── Attribution types ───────────────────────────────────────────────────────

/// Origin of a relay hint in the routing provenance record.
///
/// Parallel to `HintSource` (in `interest.rs`) but retains only the event id
/// — the tag key and position are routing-internal and don't need to surface
/// in diagnostics.
///
/// D0: carries `EventId` (hex string) — no display nouns.
#[derive(Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum HintOrigin {
    /// Hint encoded in an event tag — carries the originating event id.
    EventTag { event_id: EventId },
    /// Hint observed as the provenance relay for a prior event — carries the
    /// originating event id.
    Provenance { event_id: EventId },
    /// Hint from user/app configuration — no originating event id.
    UserConfigured,
}

/// Per-interest, per-relay attribution slice: which interest routed here,
/// which kinds it carried, and which authors the planner assigned to this relay
/// for this interest.
///
/// Populated from `RelayEntry::base_shape.kinds` and `authors_for_relay` at
/// partition time; preserved through Stage-3 merge.
///
/// D0: carries `u32` kinds + `Pubkey` — no display nouns.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterestAttribution {
    /// The interest that contributed this routing slice.
    pub interest_id: InterestId,
    /// Kinds the interest carried for this relay.
    pub kinds: BTreeSet<u32>,
    /// Authors the planner assigned to this relay for this interest. Empty
    /// for no-author interests (hashtag firehose, Case D).
    pub authors: BTreeSet<Pubkey>,
}

/// Per-relay routing provenance retained for diagnostics. Computed during
/// partition and preserved through Stage-3 merge alongside `role_tags`. Pruned
/// in lockstep with `apply_selection_with_lookup` so it matches the standing
/// plan.
///
/// D0: carries `u32` kinds + `Pubkey`/`RelayUrl` — no display nouns.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayAttribution {
    /// `UserConfigured` sub-categories that placed this relay (AppRelay,
    /// AccountRead/Write, Indexer, Bootstrap, Debug).
    pub user_configured: BTreeSet<UserConfiguredCategory>,
    /// NIP-65 outbox: authors whose write-relay set includes this relay.
    /// Tracked separately from `user_configured` authors so diagnostics can
    /// report "Outbox of N people" exactly.
    pub outbox_authors: BTreeSet<Pubkey>,
    /// Relay-hint origins that pointed here (event id when known).
    pub hints: BTreeSet<HintOrigin>,
    /// Per-interest provenance: (interest_id, kinds, authors) routed to this
    /// relay. One entry per `RelayEntry` (one per interest).
    pub interests: Vec<InterestAttribution>,
}

// ─── SubShape ────────────────────────────────────────────────────────────────

/// A single merged filter that will be emitted as one wire REQ.
///
/// The wire-emitter renders each `SubShape` as exactly one `["REQ", sub_id, filter]`
/// frame. The `canonical_filter_hash` provides stable identity for ADR-0072
/// `WireSubscriptionStatus` records across re-emissions.
///
/// # Wire-emitter lifecycle field
/// Add `lifecycle: InterestLifecycle` to this struct when the wire-emitter lands.
/// The compiler already computes lifecycle during the Stage 3 greedy merge;
/// lifecycle equality is enforced by Rule 6 before any two shapes are merged.
/// The wire-emitter needs lifecycle to decide whether to send CLOSE on EOSE
/// (`OneShot`) or keep the subscription open (Tailing).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubShape {
    /// The canonical, post-merge filter description.
    pub shape: InterestShape,
    /// All logical interests whose filters were merged into this sub-shape.
    pub originating_interests: Vec<InterestId>,
    /// Canonical hash of the serialised `shape` for stable wire-subscription identity.
    /// Format is 16 lowercase hex chars (full 64-bit FNV-1a digest); see [`canonical_filter_hash`].
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
    /// codex review at `docs/retired/removed-documents.md` (P1 plan-identity
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
/// 32-byte BLAKE3 hex form; the sixteen-character window will widen accordingly.
/// All callers (compiler, planner gate, wire-emitter, watermark store) read
/// this single helper so the swap is one edit.
#[must_use]
pub fn canonical_filter_hash(shape: &InterestShape) -> String {
    let hash = serde_json::to_string(shape).map_or_else(
        |_| stable_hash64("canonical-filter-invalid-json"),
        |json| stable_hash64(("canonical-filter", json)),
    );
    format!("{:016x}", hash)
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
    /// Per-relay routing provenance. Computed during partition and preserved
    /// through Stage-3 merge alongside `role_tags`. Pruned in lockstep with
    /// `apply_selection_with_lookup` so it always matches the standing plan.
    #[serde(default)]
    pub attribution: RelayAttribution,
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
    #[must_use]
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
#[path = "plan/tests.rs"]
mod tests;
