//! GC / hot-set budget and reporting types.
//!
//! D8: GC ceiling defaults (1000 per-view, 20000 global pinned) are documented
//! here. See `docs/design/lmdb/gc.md` §2 for the full ceiling policy.

// ─── ClaimerId ───────────────────────────────────────────────────────────────

/// Opaque view-handle id assigned by the actor (monotonically increasing u64).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ClaimerId(pub u64);

// ─── GcBudget / GcReport ─────────────────────────────────────────────────────

/// Production hot-set event ceiling (`docs/design/lmdb/gc.md` §2: default 10,000).
///
/// NOTE: `GcBudget::production()` intentionally does NOT use this constant yet.
/// `EventStore::claim/release` have zero production callers as of v0.3.0, so the
/// LRU ceiling would evict events live projections reference (V-117).  Once
/// store-claims are wired from the actor (see GitHub issue #1090), restore
/// `production().max_total_events = HOT_EVENT_CEILING`.
///
/// The constant is retained so the eviction CODE and its tests continue to
/// compile and remain exercisable.
pub const HOT_EVENT_CEILING: usize = 10_000;

/// Production per-step event budget (`docs/design/lmdb/gc.md` §3).
pub const GC_MAX_EVENTS_PER_STEP: usize = 2_000;

/// Production per-step wall-time budget in milliseconds (`docs/design/lmdb/gc.md` §3).
///
/// The Phase-1/2 loops check `start.elapsed() >= max_duration_ms` between events
/// and break early; remaining work is picked up on the next tick (every reap is
/// its own transaction, so no state corruption — `gc.md` §6).
pub const GC_MAX_DURATION_MS: u32 = 50;

/// Budget for one `gc_step()` call.
///
/// [`GcBudget::default`] uses the design-doc schedule values
/// (`max_events_per_step = 2000`, `max_duration_ms = 50`) with
/// `max_total_events = usize::MAX` (LRU eviction disabled — backward-compatible).
/// [`GcBudget::production`] is the on-device call-site budget: identical scan
/// bounds but with `max_total_events = HOT_EVENT_CEILING` so LRU eviction runs.
/// See `docs/design/lmdb/gc.md` §3.
#[derive(Clone, Copy, Debug)]
pub struct GcBudget {
    pub max_events_per_step: usize,
    pub max_duration_ms: u32,
    /// LRU eviction ceiling: if the store holds more events than this,
    /// `gc_step` evicts least-recently-accessed events (by access-sequence counter)
    /// down to this ceiling.  Only un-pinned (unclaimed) events are eligible.
    ///
    /// `GcBudget::default()` leaves this `usize::MAX` (disabled);
    /// [`GcBudget::production`] sets it to [`HOT_EVENT_CEILING`] to cap store size.
    pub max_total_events: usize,
}

impl Default for GcBudget {
    /// Design-doc schedule values (`gc.md` §3) with LRU eviction left disabled.
    ///
    /// This is the single source of truth for the `2000 / 50ms` scan bounds the
    /// doc quotes. The production call site uses [`GcBudget::production`], which
    /// reuses these bounds and adds the finite LRU ceiling.
    fn default() -> Self {
        Self {
            max_events_per_step: GC_MAX_EVENTS_PER_STEP,
            max_duration_ms: GC_MAX_DURATION_MS,
            max_total_events: usize::MAX,
        }
    }
}

impl GcBudget {
    /// The on-device production budget used by the actor's 60-second idle-tick gc pass.
    ///
    /// LRU ceiling is intentionally **disabled** (`max_total_events = usize::MAX`) until
    /// `EventStore::claim/release` have production callers.  Without claims wired, a
    /// finite ceiling silently evicts events that live projections reference (V-117).
    /// See GitHub issue #1090 for the wiring work that re-enables the ceiling.
    ///
    /// The eviction code (Phase 2 in `lmdb/gc.rs`) is kept and tested via unit tests
    /// that pass an explicit finite ceiling — it is only the production budget that
    /// is temporarily reverted here.
    pub fn production() -> Self {
        // max_total_events = usize::MAX disables Phase-2 LRU eviction.
        // Re-enable with `max_total_events: HOT_EVENT_CEILING` once #1090 is done.
        Self::default()
    }
}

/// Report produced by `gc_step()`.
#[derive(Clone, Debug, Default)]
pub struct GcReport {
    pub expired_reaped: usize,
    pub lru_evicted: usize,
    /// Per-id tombstone rows purged (origin: Kind5, NIP40Expiry, AdminPurge).
    pub tombstones_purged: usize,
    /// Address-keyed tombstone rows purged (kind:5 `a`-tag deletes).
    pub addr_tombstones_purged: usize,
    pub duration_ms: u32,
}

// ─── Filters ─────────────────────────────────────────────────────────────────

use super::ids::{EventId, PubKey, RelayUrl};

/// NMP-internal delete filter — NOT a pass-through to `nostr::Filter`.
/// Only exposes operations the kernel legitimately needs; does not allow
/// arbitrary remote filters as a delete vector.
#[derive(Clone, Debug)]
pub enum DeleteFilter {
    /// All events sourced exclusively from this relay.
    ByRelayOnly(RelayUrl),
    /// All events by a specific pubkey.
    ByAuthor(PubKey),
    /// Specific event ids.
    ByIds(Vec<EventId>),
    /// All events with kind in `[lo, hi]` (inclusive range).
    ByKindRange { lo: u32, hi: u32 },
}

// ─── Export ──────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
pub enum DumpFormat {
    Jsonl,
}

#[derive(Clone, Debug, Default)]
pub struct DumpStats {
    pub events: u64,
    pub tombstones: u64,
    pub watermarks: u64,
    pub domain_rows: u64,
    pub bytes_written: u64,
}
