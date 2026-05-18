//! GC / hot-set budget and reporting types.
//!
//! D8: GC ceiling defaults (1000 per-view, 20000 global pinned) are documented
//! here. See `docs/design/lmdb/gc.md` §2 for the full ceiling policy.

// ─── ClaimerId ───────────────────────────────────────────────────────────────

/// Opaque view-handle id assigned by the actor (monotonically increasing u64).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ClaimerId(pub u64);

// ─── GcBudget / GcReport ─────────────────────────────────────────────────────

/// Budget for one `gc_step()` call.
///
/// Defaults: `max_events_per_step = 2000`, `max_duration_ms = 50`.
/// See `docs/design/lmdb/gc.md` §3.
#[derive(Clone, Copy, Debug)]
pub struct GcBudget {
    pub max_events_per_step: usize,
    pub max_duration_ms: u32,
}

/// Report produced by `gc_step()`.
#[derive(Clone, Debug, Default)]
pub struct GcReport {
    pub expired_reaped: usize,
    pub lru_evicted: usize,
    pub tombstones_purged: usize,
    pub duration_ms: u32,
}

// ─── Filters ─────────────────────────────────────────────────────────────────

use super::ids::{EventId, PubKey, RelayUrl};

/// NMP-internal delete filter — NOT a pass-through to nostr::Filter.
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
