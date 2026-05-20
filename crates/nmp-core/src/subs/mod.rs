//! M8-subs — subscription lifecycle: the seam between M2's `CompiledPlan`
//! and the wire.
//!
//! See `docs/plan/m8-subscription-lifecycle.md` for the scope discriminator
//! against M4 (negentropy), M5 (NIP-42 auth), M7 (publishing). This module
//! ships only the **seams**:
//!
//! - [`InterestRegistry`] — single-writer logical-interest store (D4).
//! - [`CompileTrigger`] inbox — FIFO + per-tick coalescing (D8).
//! - Wire-emitter — `CompiledPlan` → `Vec<WireFrame>` diff.
//! - [`ConnectionPool`] — uniform send-path shared by M4/M5/M7 (D7).
//!
//! Doctrine:
//! - **D3** routing is consumed verbatim from the planner; we never reroute.
//! - **D4** the registry is the single writer of the active-interest set.
//! - **D6** all error types here are internal `Result`s; no FFI exposure.
//! - **D7** the pool reports; the actor decides. No reconnect policy here.
//! - **D8** per-tick coalesce caps recompiles at 1 / view / tick.
//!
//! Design: `docs/design/subscription-compilation/recompilation.md` §4.
//!
//! ## Module layout (NMP #169 file-size-gate split)
//!
//! `SubscriptionLifecycle`'s struct definition lives here in the module root
//! so this file owns the privacy boundary; its inherent `impl` is split
//! across sibling child modules with **zero behavioural change**:
//!
//! - [`lifecycle`] — constructor, `Default`, simple accessors/setters.
//! - [`recompile`] — `recompile_and_diff`, `drain_tick`, T129 watermark
//!   rewrite free fns.
//! - [`handlers`] — reconnect / EOSE / deadline / auth-state handlers and
//!   the `current_plan_*` diagnostic accessors.
//!
//! Child modules see the struct's private fields (a child module can read
//! its parent's private items), so no field needed widened visibility. The
//! public API path (`crate::subs::SubscriptionLifecycle`, the `pub` type
//! aliases / consts, and the `pub use` re-exports below) is unchanged.

pub(crate) mod auth_gate;
pub(crate) mod inbox;
pub(crate) mod lifecycle_gate;
pub(crate) mod oneshot;
pub(crate) mod pool;
pub(crate) mod registry;
pub(crate) mod sub_key;
pub(crate) mod trigger;
pub(crate) mod unknown_ids;
pub(crate) mod wire;

mod handlers;
mod lifecycle;
mod recompile;

#[cfg(test)]
mod coverage_hook_tests;
#[cfg(test)]
mod discovery_tests;
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod since_rewrite_tests;

use std::collections::BTreeSet;
use std::sync::Arc;

use auth_gate::AuthGate;
use lifecycle_gate::LifecycleGate;

use crate::planner::{CompiledPlan, InterestShape, RelayUrl};

pub use inbox::TriggerInbox;
pub use oneshot::{OneshotApi, OneshotToken};
pub use pool::{ConnectionPool, InMemoryPool, PoolSendOutcome};
pub use registry::InterestRegistry;
pub use sub_key::{SubIdentity, SubKey, SubKeyBuilder, SubOwnerKey, SubScope};
pub use trigger::{AccountId, CompileTrigger, InvalidateReason, RelayAuthState, SignerId};
pub use unknown_ids::UnknownIds;
pub use wire::{plan_diff, WireFrame};

/// Post-compile plan-mutation hook (M4 negentropy coverage gate).
///
/// The lifecycle owns the *seam* into which `nmp-nip77`'s
/// `apply_coverage_filter` is installed by the actor at startup. The hook runs
/// between `compile()` and `plan_diff()` — i.e. after the M2 compiler
/// produces the plan but before the wire-emitter diffs against the prior
/// plan. The hook is free to drop sub-shapes, bump `since`, or otherwise
/// rewrite the plan; any sub-shape whose `shape` is mutated MUST call
/// [`crate::planner::SubShape::recompute_hash`] (see the M4 codex review's P1
/// finding in `docs/perf/codex-reviews/076173d.md`).
///
/// Direction: `nmp-core` defines the seam, `nmp-nip77` installs the policy —
/// keeping coverage-gate / NIP-77 vocabulary out of `nmp-core` per D0
/// ("kernel never grows app nouns").
///
// TODO(D2): `coverage_hook` is NEVER installed in the production kernel.
//
// The single production assembly site is `Kernel::with_publish_store`
// (`crates/nmp-core/src/kernel/mod.rs:535`): it constructs the
// `SubscriptionLifecycle` and calls `set_watermark_fn`, but it never calls
// `SubscriptionLifecycle::set_coverage_hook`. Neither `actor::run_actor` nor
// the `nmp-core/src/ffi` app surface installs it either. The only real wiring
// of `nmp_nip77::apply_coverage_filter` lives in
// `nmp-testing/tests/framework_magic_c10.rs` (a test).
//
// Consequence: the shipping kernel does NOT enforce D2 ("negentropy before
// REQ") — every plan flows straight to a raw REQ. D2 is therefore
// CONVENTION-ONLY, not a type-system or assembly invariant.
//
// This cannot be fixed structurally from inside `nmp-core`: the hook policy
// lives in `nmp-nip77`, which already depends on `nmp-core`, so a
// `nmp-core → nmp-nip77` dep is both a D0 app-noun leak AND a dependency
// cycle. Enforcing D2 structurally requires a HIGHER-LEVEL assembly crate
// that can depend on both `nmp-core` and `nmp-nip77` and installs the hook at
// kernel-construction time. No such crate currently exists. (The sibling
// `set_watermark_fn` seam is store-backed and lives entirely inside
// `nmp-core`, so T129 IS wired in `Kernel::with_publish_store` — only this
// coverage hook remains unwired.)
//
// Open tracking item: the `#[ignore]`d sentinel test
// `subs::coverage_hook_tests::d2_production_kernel_installs_coverage_hook`.
// 2026-05-20 D2 audit.
pub type PlanCoverageHook = Arc<dyn Fn(&mut CompiledPlan) + Send + Sync>;

/// T129 watermark resolver — returns the most-recent stored `created_at`
/// (unix seconds) for events matching `shape`, or `None` when the store has
/// no matching events.
///
/// Installed by the kernel via [`SubscriptionLifecycle::set_watermark_fn`].
/// The kernel is the only legitimate caller — view modules and tests inject a
/// stub closure. The kernel-side closure translates the shape into a
/// `StoreQuery` (`AuthorKind` when authors+kinds are scoped, otherwise
/// `KindTime`) and invokes `EventStore::query_visit` with `limit = 1`, which
/// early-stops at the newest stored match on the relevant secondary index.
///
/// The trait-object signature keeps `nmp-core::subs` independent of any
/// concrete store type (D8: zero per-emit alloc, dispatch is a single vtable
/// lookup; the closure itself reuses the index buffers underlying
/// `query_visit`).
pub type WatermarkFn = Arc<dyn Fn(&InterestShape) -> Option<u64> + Send + Sync>;

/// Default upper bound on concurrent relay connections after greedy
/// max-coverage reduction. Mirrors the `outbox_perf` example budget.
pub const DEFAULT_SELECT_MAX_CONNECTIONS: usize = 30;

/// Default per-author redundancy cap (applesauce-pure). Each follow is
/// covered by at most this many surviving relays.
pub const DEFAULT_SELECT_MAX_PER_USER: usize = 2;

/// Max pubkeys per implicit kind:10002 discovery REQ.
///
/// 500 (not the kernel's conservative `DISCOVERY_BATCH = 50`): a 50-author
/// batch turns a ~1000-follow cold start into ~20 separate REQs blasted at
/// one indexer in a burst — exactly the pattern that triggers relay
/// rate-limiting (observed: purplepag.es answering AUTH + CLOSED
/// "rate limit exceeded"). 500 collapses the same cold start to ~2 REQs.
/// Mainstream relays (damus, nos.lol, primal, strfry-based) accept
/// author filters in the hundreds; a relay that truncates a large filter
/// degrades gracefully (the still-unknown authors stay in
/// `probed_mailboxes` unprobed-successfully and a later `refresh` retries).
/// Fewer REQs ≫ marginally-wider filter risk.
const MAILBOX_PROBE_BATCH: usize = 500;

// ─── SubscriptionLifecycle ───────────────────────────────────────────────────

/// The top-level subscription lifecycle controller.
///
/// Owns the registry, trigger inbox, last-compiled plan, the lifecycle gate
/// (OneShot / BoundedTime CLOSE bookkeeping), and the auth gate (REQs to
/// auth-paused relays held in a pending buffer). Drives recompiles when
/// ticked; emits `WireFrame`s for the actor to push through the connection
/// pool.
///
/// **Per-tick discipline (D8):** N triggers in the inbox between two
/// `drain_tick()` calls produce at most one compile. An empty inbox tick
/// produces zero compiles.
///
/// The inherent `impl` is split across the `lifecycle` / `recompile` /
/// `handlers` sibling child modules (NMP #169); the struct definition stays
/// here so the privacy boundary is owned by the module root.
pub struct SubscriptionLifecycle {
    registry: InterestRegistry,
    inbox: TriggerInbox,
    indexer_relays: Vec<RelayUrl>,
    /// Operator-configured app relays (T134).
    ///
    /// Threaded into the compiler on every recompile so author REQs ride
    /// the additive `UserConfigured(AppRelay)` lane on top of NIP-65 (or
    /// substitute when NIP-65 is unknown). Set via [`Self::set_app_relays`];
    /// defaults to empty so legacy lifecycle tests stay green.
    app_relays: Vec<RelayUrl>,
    /// Active account read relays — for no-author/no-address interests
    /// (hashtag firehose, global search). Set via
    /// [`Self::set_active_account_read_relays`]; defaults to empty so the
    /// no-author firehose falls back to `app_relays`, then indexer.
    active_account_read_relays: Vec<RelayUrl>,
    /// The plan currently believed-to-be-live on the wire.
    current_plan: Option<CompiledPlan>,
    /// Per-sub lifecycle bookkeeping (OneShot, BoundedTime).
    lifecycle_gate: LifecycleGate,
    /// Per-relay auth state + pending REQ buffer.
    auth_gate: AuthGate,
    /// Monotonic compile counter for test assertions.
    compile_count: u64,
    /// Optional post-compile plan-mutation hook (see [`PlanCoverageHook`]).
    /// Set via [`Self::set_coverage_hook`]; absent by default so the kernel
    /// links cleanly without any NIP-77 dependency.
    coverage_hook: Option<PlanCoverageHook>,
    /// T129 — optional watermark resolver. Installed by the kernel from the
    /// `EventStore` at startup; tests inject a stub closure. When set,
    /// [`Self::recompile_and_diff`] rewrites each non-ephemeral sub-shape's
    /// `since` to `max(existing_since, watermark + 1)` so the relay REQ does
    /// not re-fetch events already on disk. See module doc on [`WatermarkFn`]
    /// and the seam rationale documented in `planner/mod.rs`.
    watermark_fn: Option<WatermarkFn>,
    /// Greedy max-coverage budget — upper bound on concurrent relay
    /// connections after [`crate::planner::apply_selection`] reduces the
    /// naive plan.
    ///
    /// The naive M2 plan connects to every NIP-65 write relay declared by
    /// every follow (in real test data: 287 relays for 1048 follows). The
    /// selector reduces this to ~`select_max_connections` while preserving
    /// per-author coverage via [`Self::select_max_per_user`]. Default:
    /// [`DEFAULT_SELECT_MAX_CONNECTIONS`] (matches the `outbox_perf`
    /// example). Tune via [`Self::set_selection_budget`].
    select_max_connections: usize,
    /// Per-author redundancy cap — each follow may be served by at most
    /// this many surviving relays. Prevents the greedy algorithm from
    /// spending its whole connection budget on the popularity-distribution
    /// head while ignoring the long tail. Default:
    /// [`DEFAULT_SELECT_MAX_PER_USER`] (applesauce-pure).
    select_max_per_user: usize,
    /// Relays considered persistently unreachable. Filtered out of the plan
    /// BEFORE [`crate::planner::apply_selection`] runs, so the selector picks
    /// alternative NIP-65 write relays for the affected authors. Populated by
    /// the actor via [`Self::mark_relay_dead`] in response to repeated connect
    /// failures (heuristic owned by the caller — the lifecycle just respects
    /// the set). Cleared per-relay via [`Self::mark_relay_alive`] on a
    /// successful re-connection. Each transition fires
    /// [`CompileTrigger::RelayHealthChanged`] so the affected authors re-route
    /// on the next compile pass.
    dead_relays: BTreeSet<RelayUrl>,
    /// Pubkeys for which a kind:10002 discovery REQ has already been emitted
    /// this session. Implicit-discovery dedup: when `recompile_and_diff`
    /// compiles a REQ that targets an author with no cached mailbox AND not
    /// in this set, it auto-emits a `kinds:[10002]` discovery REQ to the
    /// indexer set and records the author here.
    ///
    /// **Insert-only for the session** (no TTL). An author who has never
    /// published a kind:10002 is probed exactly once; the empty EOSE that
    /// comes back leaves them in this set so subsequent recompiles do not
    /// re-probe (the "nor have tried" half of the contract). Cleared in bulk
    /// via [`Self::clear_probed_mailboxes`] (the `refresh` escape hatch).
    /// A relay-list that *does* arrive lands in the mailbox cache and fires
    /// [`CompileTrigger::Nip65Arrived`], re-routing the author via NIP-65 —
    /// the probed mark is then moot (the cache hit short-circuits the
    /// unknown-author check before this set is consulted).
    probed_mailboxes: BTreeSet<String>,
    /// T140 (D6 / codex finding #7): the most recent *genuine* planner error
    /// from [`Self::drain_tick`].
    ///
    /// `drain_tick` previously mapped every `Err(_)` to `Vec::new()` via
    /// `unwrap_or_default()` — a silent swallow on a path that is now
    /// FFI-visible (the actor idle loop drives it). D6 forbids silently
    /// discarding errors. `EmptyInterestSet` is a benign steady state (no
    /// interests → empty diff) and is NOT recorded here; structural errors
    /// (`InvalidShape`, `HashingFailed`) ARE recorded so an operator /
    /// diagnostic surface can observe them. `None` until the first genuine
    /// error; never auto-cleared (latest-error-wins).
    last_planner_error: Option<String>,
}
