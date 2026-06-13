//! GC / hot-set budget and reporting types.
//!
//! D8: GC ceiling defaults (1000 per-view, 20000 global pinned) are documented
//! here. See `docs/design/lmdb/gc.md` §2 for the full ceiling policy.

// ─── GcBudget / GcReport ─────────────────────────────────────────────────────

/// Production hot-set event ceiling (`docs/design/lmdb/gc.md` §2: default 10,000).
///
/// [`GcBudget::production`] sets `max_total_events = HOT_EVENT_CEILING` so the
/// on-device GC pass evicts least-recently-accessed un-pinned events down to
/// this ceiling (#1090 Stage 3). Floor-coherence is guaranteed by
/// `Kernel::derive_store_pin_set`, which pins every stored event at or below
/// each active floored shape's `since`-floor (#1090 Stage 2) so LRU eviction
/// cannot punch a hole the floored self-healing REQ will never re-request.
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
/// `max_total_events = usize::MAX` (LRU eviction disabled — used by tests that
/// want only expiry/tombstone reaping, never on-device).
/// [`GcBudget::production`] is the on-device call-site budget: identical scan
/// bounds, but `max_total_events = HOT_EVENT_CEILING` so the LRU ceiling is
/// enforced (#1090 Stage 3).
/// See `docs/design/lmdb/gc.md` §3.
#[derive(Clone, Copy, Debug)]
pub struct GcBudget {
    pub max_events_per_step: usize,
    pub max_duration_ms: u32,
    /// LRU eviction ceiling: if the store holds more events than this,
    /// `gc_step` evicts least-recently-accessed events (by access-sequence counter)
    /// down to this ceiling.  Only un-pinned (unclaimed) events are eligible.
    ///
    /// [`GcBudget::default()`] leaves this `usize::MAX` (eviction disabled) for
    /// tests; [`GcBudget::production`] sets it to [`HOT_EVENT_CEILING`]. The
    /// kernel-derived pin set passed to [`EventStore::gc_step_with_pins`]
    /// (#1090 Stage 1) plus floor-coherent pinning (#1090 Stage 2) guarantee no
    /// live working-set or floored-shape event is eligible for eviction.
    pub max_total_events: usize,
}

impl Default for GcBudget {
    /// Design-doc schedule values (`gc.md` §3) with LRU eviction left disabled.
    ///
    /// This is the single source of truth for the `2000 / 50ms` scan bounds the
    /// doc quotes. The production call site uses [`GcBudget::production`], which
    /// keeps these scan bounds but enables the finite LRU ceiling.
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
    /// Same scan bounds as [`GcBudget::default`], with the LRU ceiling enabled
    /// (`max_total_events = HOT_EVENT_CEILING`). Floor-coherence is guaranteed by
    /// `Kernel::derive_store_pin_set` (#1090 Stage 2), so eviction never punches a
    /// hole below an active floored shape's `since`-floor.
    pub fn production() -> Self {
        Self {
            max_total_events: HOT_EVENT_CEILING,
            ..Self::default()
        }
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
