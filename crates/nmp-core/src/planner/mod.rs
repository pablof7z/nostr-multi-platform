//! Subscription compiler — the M2 planner subsystem.
//!
//! Turns a set of `LogicalInterest`s into a `CompiledPlan` mapping each
//! relay URL to the exact set of REQ frames to emit.
//!
//! ## Module structure
//!
//! - `interest`  — `LogicalInterest`, `InterestShape`, `NaddrCoord` types.
//! - `lattice`   — `merge()` function implementing the 9 merge rules.
//! - `compiler`  — 4-stage pipeline: resolve → fallback → merge → plan-id.
//! - `plan`      — `CompiledPlan`, `RelayPlan`, `SubShape`, `RoutingSource`.
//!
//! ## Usage (phase 1)
//!
//! ```rust,ignore
//! use nmp_core::planner::{
//!     compiler::{InMemoryMailboxCache, MailboxSnapshot, SubscriptionCompiler},
//!     interest::{InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest},
//! };
//!
//! let cache = InMemoryMailboxCache::new();
//! let indexer = vec!["wss://purplepag.es".to_string()];
//! let compiler = SubscriptionCompiler::new(&cache, &indexer);
//! let plan = compiler.compile(&[interest])?;
//! ```
//!
//! ## Doctrine compliance
//!
//! - **D3** — outbox routing is automatic; view modules never name relay URLs.
//! - **D6** — `PlannerError` is an internal `Result`; it never crosses FFI.
//!   Map to a toast string at the actor boundary.
//! - **D8** — the hot path (merge lattice) uses only stack-allocated comparisons
//!   after the initial interest registration.
//!
//! ## Plan-id determinism vs. post-compile mutators
//!
//! `plan_id` (and `SubShape::canonical_filter_hash`) is content-addressed over
//! the **interest set + mailbox snapshot + lattice version** only — never over
//! runtime state. Post-compile passes that bump `since` therefore split into
//! two camps:
//!
//! - **Coverage gate (M4 / NIP-77)**: mutating `since` changes what content
//!   the relay should send (skipping authoritative ranges already on disk).
//!   The wire-emitter MUST see a new sub-id so the new REQ goes out — the
//!   hook calls [`plan::SubShape::recompute_hash`] after each mutation.
//! - **`addSinceFromCache` (T129)**: bumping `since` is a no-data-loss floor
//!   — every event the relay would have sent below `watermark + 1` is
//!   already on disk; not seeing them again is the *point*. The wire-emitter
//!   MUST NOT see a new sub-id (else every recompile churns CLOSE+REQ as
//!   the watermark advances). The rewrite therefore leaves
//!   `canonical_filter_hash` alone and is applied AFTER the hash is computed.
//!
//! Both rules collapse to the same invariant: `canonical_filter_hash` reflects
//! "what does this filter mean structurally?", not "what's currently on the
//! wire?". The wire-emitter's diff is the only place runtime state crosses
//! into the emitted frames.
//!
//! Design: `docs/design/subscription-compilation/`

pub(crate) mod compiler;
pub(crate) mod interest;
pub(crate) mod lattice;
pub(crate) mod plan;

// ─── Public API surface ──────────────────────────────────────────────────────
//
// Only the items below cross the crate boundary. Internals (RelayEntry,
// partition_interest, FnvHasher, rule*_* functions, etc.) stay module-private.
// `lattice::merge` is re-exported for the nmp-testing audit gate; all others
// are consumed by crate-internal callers (kernel, actor).

pub use compiler::{
    CompileContext,
    EmptyMailboxCache,
    InMemoryMailboxCache,
    MailboxCache,
    MailboxSnapshot,
    SubscriptionCompiler,
};
pub use interest::{
    HintSource,
    InterestId,
    InterestLifecycle,
    InterestScope,
    InterestShape,
    LogicalInterest,
    NaddrCoord,
    Pubkey,
    RelayHint,
    RelayUrl,
};
pub use lattice::{merge, MergeOutcome};
pub use plan::{
    canonical_filter_hash,
    CompiledPlan,
    PlannerError,
    RelayPlan,
    RoutingSource,
    SubShape,
    UserConfiguredCategory,
};
