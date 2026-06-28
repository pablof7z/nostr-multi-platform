//! GC / hot-set budget and reporting types.
//!
//! D8: GC ceiling defaults (1000 per-view, 20000 global pinned) are documented
//! here. See `docs/design/lmdb/gc.md` §2 for the full ceiling policy.

// ─── GcBudget / GcReport ─────────────────────────────────────────────────────

/// Default explicit durable-retention ceiling (`docs/design/lmdb/gc.md` §3).
///
/// This is no longer used by [`GcBudget::production`]. Production GC bounds RAM
/// working-set caches separately and leaves durable event rows unbounded unless
/// a caller deliberately opts into finite durable retention with
/// [`GcBudget::with_durable_event_ceiling`]. The retained constant keeps the
/// historical 10,000-event policy available for explicit tests/configuration.
pub const DEFAULT_DURABLE_EVENT_CEILING: usize = 10_000;

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
/// `max_total_events = usize::MAX` (durable LRU deletion disabled).
/// [`GcBudget::production`] is intentionally identical: on-device GC reaps
/// correctness deletes and tombstones, while RAM working-set eviction is handled
/// by the kernel's RAM-cache pass. A finite durable row ceiling must be explicit.
/// See `docs/design/lmdb/gc.md` §3.
#[derive(Clone, Copy, Debug)]
pub struct GcBudget {
    pub max_events_per_step: usize,
    pub max_duration_ms: u32,
    /// Durable LRU eviction ceiling: if the store holds more events than this,
    /// `gc_step` evicts least-recently-accessed events (by access-sequence counter)
    /// down to this ceiling. Only un-pinned events are eligible.
    ///
    /// [`GcBudget::default()`] and [`GcBudget::production`] leave this
    /// `usize::MAX`. Use [`GcBudget::with_durable_event_ceiling`] to opt into a
    /// finite durable-retention policy. The kernel-derived pin set passed to
    /// [`EventStore::gc_step_with_pins`] plus floor-coherent pinning keep any
    /// explicit durable deletion from punching holes in active covered ranges.
    pub max_total_events: usize,
}

impl Default for GcBudget {
    /// Design-doc schedule values (`gc.md` §3) with LRU eviction left disabled.
    ///
    /// This is the single source of truth for the `2000 / 50ms` scan bounds the
    /// doc quotes. Production uses the same budget shape and leaves durable LRU
    /// deletion disabled.
    fn default() -> Self {
        Self {
            max_events_per_step: GC_MAX_EVENTS_PER_STEP,
            max_duration_ms: GC_MAX_DURATION_MS,
            max_total_events: usize::MAX,
        }
    }
}

impl GcBudget {
    /// The on-device production budget used by the actor's 60-second idle-tick GC pass.
    ///
    /// Same scan bounds as [`GcBudget::default`], with durable LRU deletion
    /// disabled. RAM working-set pressure is handled by `Kernel::evict_ram_caches`;
    /// durable event retention must be an explicit disk/user policy.
    pub fn production() -> Self {
        Self::default()
    }

    /// Budget for an explicit finite durable-retention policy.
    ///
    /// This preserves the existing guarded durable-LRU machinery for tests and
    /// future user/disk quota policy without making valid fetched events expire
    /// from the default production store.
    pub fn with_durable_event_ceiling(max_total_events: usize) -> Self {
        Self {
            max_total_events,
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
