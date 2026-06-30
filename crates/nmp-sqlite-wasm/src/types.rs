//! Crate-local value types for the PR-5 engine surface (#1007).
//!
//! GC budgets/reports, the delete filter, dump stats, the replaceable-freshness
//! key, interaction counts, and the domain-migration staging buffer — each a
//! field-for-field mirror of its `nmp_store` counterpart (`types::gc`,
//! `types::outcomes`, `domain_migration`, `ReplaceableKey`). The crate cannot
//! depend on `nmp-store` (Cargo cycle — see the crate-level docs), so the
//! `nmp-store` `EventStore` wrapper maps these 1:1 at the cycle-free seam,
//! exactly as it does the [`crate::outcome`] insert-outcome types.
//!
//! Pure and target-agnostic: these are plain data, no shim, so the small bits of
//! logic on them (defaults, `ReplaceableKey::kind`) are unit-tested on native.

use crate::outcome::{EventId, PubKey};

// ─── GC budget / report (mirror nmp_store::types::gc) ───────────────────────────

/// Production per-step event budget (`docs/design/lmdb/gc.md` §3).
pub const GC_MAX_EVENTS_PER_STEP: usize = 2_000;
/// Production per-step wall-time budget in milliseconds.
pub const GC_MAX_DURATION_MS: u32 = 50;

/// Budget for one [`crate::OpfsSqliteStore::gc_step_with_pins`] call.
///
/// [`GcBudget::default`] uses the design-doc schedule values
/// (`max_events_per_step = 2000`, `max_duration_ms = 50`) with
/// `max_total_events = usize::MAX` (durable LRU deletion disabled — RAM
/// working-set eviction is the kernel's job; a finite durable ceiling is an
/// explicit opt-in).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GcBudget {
    /// Maximum events touched in one pass (per phase).
    pub max_events_per_step: usize,
    /// Soft wall-clock ceiling for the pass, milliseconds.
    pub max_duration_ms: u32,
    /// Durable LRU eviction ceiling. When the store holds more events than this,
    /// `gc_step` evicts least-recently-accessed un-pinned events down to it.
    /// `usize::MAX` disables durable eviction (the default).
    pub max_total_events: usize,
}

impl Default for GcBudget {
    fn default() -> Self {
        Self {
            max_events_per_step: GC_MAX_EVENTS_PER_STEP,
            max_duration_ms: GC_MAX_DURATION_MS,
            max_total_events: usize::MAX,
        }
    }
}

impl GcBudget {
    /// The on-device production budget (identical to [`Self::default`]: durable
    /// LRU deletion disabled).
    #[must_use]
    pub fn production() -> Self {
        Self::default()
    }

    /// Budget for an explicit finite durable-retention policy.
    #[must_use]
    pub fn with_durable_event_ceiling(max_total_events: usize) -> Self {
        Self {
            max_total_events,
            ..Self::default()
        }
    }
}

/// Report produced by one GC pass (mirror of `nmp_store::GcReport`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GcReport {
    /// NIP-40 expired events reaped this pass.
    pub expired_reaped: usize,
    /// Un-pinned events LRU-evicted this pass.
    pub lru_evicted: usize,
    /// Per-id tombstone rows purged (aged past retention).
    pub tombstones_purged: usize,
    /// Address-keyed tombstone rows purged.
    pub addr_tombstones_purged: usize,
    /// Wall-clock duration of the pass, milliseconds.
    pub duration_ms: u32,
}

// ─── Delete filter (mirror nmp_store::DeleteFilter) ─────────────────────────────

/// NMP-internal bulk-delete filter — NOT a pass-through to a remote filter.
/// Used by GC / admin purge / kind:5 application only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeleteFilter {
    /// All events sourced exclusively from this relay.
    ByRelayOnly(String),
    /// All events by a specific pubkey.
    ByAuthor(PubKey),
    /// Specific event ids.
    ByIds(Vec<EventId>),
    /// All events with kind in `[lo, hi]` (inclusive).
    ByKindRange {
        /// Inclusive low kind bound.
        lo: u32,
        /// Inclusive high kind bound.
        hi: u32,
    },
}

// ─── Dump stats (mirror nmp_store::DumpStats) ───────────────────────────────────

/// Counters returned by [`crate::OpfsSqliteStore::dump`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DumpStats {
    /// Event rows written.
    pub events: u64,
    /// Tombstone rows written.
    pub tombstones: u64,
    /// Watermark rows written (none in this engine yet — kept for parity).
    pub watermarks: u64,
    /// Domain rows written.
    pub domain_rows: u64,
    /// Total bytes written to the sink.
    pub bytes_written: u64,
}

// ─── Replaceable freshness key (mirror nmp_store::ReplaceableKey) ───────────────

/// Identity of a replaceable / addressable event, the F-TTL freshness key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ReplaceableKey {
    /// Regular replaceable: identified by kind + author pubkey.
    Regular {
        /// NIP-01 kind.
        kind: u32,
        /// Author pubkey.
        pubkey: PubKey,
    },
    /// Parameterized replaceable: kind + author pubkey + `d`-tag.
    Parameterized {
        /// NIP-01 kind.
        kind: u32,
        /// Author pubkey.
        pubkey: PubKey,
        /// `d`-tag value bytes.
        d_tag: Vec<u8>,
    },
}

impl ReplaceableKey {
    /// The kind for this key.
    #[must_use]
    pub fn kind(&self) -> u32 {
        match self {
            Self::Regular { kind, .. } | Self::Parameterized { kind, .. } => *kind,
        }
    }

    /// Encode to the `replaceable_freshness.rkey` blob:
    /// `kind(BE4) || pubkey(32) || d_tag bytes`. A regular key has no `d_tag`
    /// segment; a parameterized key with an empty `d` yields just the prefix —
    /// the encodings never collide because regular and addressable kinds occupy
    /// disjoint NIP-01 ranges.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Self::Regular { kind, pubkey } => {
                let mut k = Vec::with_capacity(4 + 32);
                k.extend_from_slice(&kind.to_be_bytes());
                k.extend_from_slice(pubkey);
                k
            }
            Self::Parameterized { kind, pubkey, d_tag } => {
                let mut k = Vec::with_capacity(4 + 32 + d_tag.len());
                k.extend_from_slice(&kind.to_be_bytes());
                k.extend_from_slice(pubkey);
                k.extend_from_slice(d_tag);
                k
            }
        }
    }
}

// Cross-protocol engagement aggregation is NOT a storage concern — it moved to
// `nmp-relations` (#2512). This engine exposes no reference-counter sidecar; the
// `nmp-store` OPFS wrapper falls back to the trait default (empty counts).

// ─── Domain migration staging (mirror nmp_store::{DomainMigration, MigrationTx}) ─

/// One domain-namespace schema migration step.
#[derive(Clone, Copy)]
pub struct DomainMigration {
    /// Version this step upgrades from.
    pub from_version: u32,
    /// Version this step upgrades to.
    pub to_version: u32,
    /// Stages the rows the migration writes into a [`MigrationTx`].
    pub apply: fn(&mut MigrationTx) -> Result<(), String>,
}

/// In-memory staging buffer a [`DomainMigration`] writes through. Keys are
/// namespace-relative (the backend prefixes the namespace before storing).
#[derive(Default)]
pub struct MigrationTx {
    writes: Vec<(Vec<u8>, Vec<u8>)>,
}

impl MigrationTx {
    /// Stage one namespace-relative `(key, value)` write.
    pub fn put(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.writes.push((key, value));
    }

    /// Staged writes, in insertion order.
    #[must_use]
    pub fn writes(&self) -> &[(Vec<u8>, Vec<u8>)] {
        &self.writes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gc_budget_defaults_match_design_schedule() {
        let b = GcBudget::default();
        assert_eq!(b.max_events_per_step, 2_000);
        assert_eq!(b.max_duration_ms, 50);
        assert_eq!(b.max_total_events, usize::MAX);
        assert_eq!(GcBudget::production(), b);
        assert_eq!(GcBudget::with_durable_event_ceiling(10).max_total_events, 10);
    }

    #[test]
    fn replaceable_key_encode_is_disjoint_and_kind_addressable() {
        let reg = ReplaceableKey::Regular {
            kind: 0,
            pubkey: [7u8; 32],
        };
        let par = ReplaceableKey::Parameterized {
            kind: 30023,
            pubkey: [7u8; 32],
            d_tag: b"slug".to_vec(),
        };
        assert_eq!(reg.kind(), 0);
        assert_eq!(par.kind(), 30023);
        assert_eq!(reg.encode().len(), 36);
        assert_eq!(par.encode().len(), 40);
        assert_ne!(reg.encode(), par.encode());
    }

    #[test]
    fn migration_tx_preserves_insertion_order() {
        let mut tx = MigrationTx::default();
        tx.put(b"a".to_vec(), b"1".to_vec());
        tx.put(b"b".to_vec(), b"2".to_vec());
        assert_eq!(tx.writes().len(), 2);
        assert_eq!(tx.writes()[0].0, b"a");
        assert_eq!(tx.writes()[1].0, b"b");
    }
}
