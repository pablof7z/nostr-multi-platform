//! Subscription compiler — the M2 planner subsystem.
//!
//! Turns a set of `LogicalInterest`s into a `CompiledPlan` mapping each
//! relay URL to the exact set of REQ frames to emit.
//!
//! ## Module structure
//!
//! - `interest`  — `LogicalInterest`, `InterestShape`, `NaddrCoord` types.
//! - `lattice`   — `merge()` function implementing the 8 merge rules.
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
    InterestId,
    InterestLifecycle,
    InterestScope,
    InterestShape,
    LogicalInterest,
    NaddrCoord,
    Pubkey,
    RelayUrl,
};
pub use lattice::MergeOutcome;
pub use plan::{CompiledPlan, PlannerError, RelayPlan, RoutingSource, SubShape, UserConfiguredCategory};
