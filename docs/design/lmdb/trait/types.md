# LMDB sub-design: supporting types for `EventStore`

> Extracted from [`../trait.md`](../trait.md) §2 to keep that file within the 300-LOC soft limit.
> These types live in `crates/nmp-core/src/store/types.rs` and are re-exported from `nmp_core::store`.

## Type aliases

```rust
use std::sync::Arc;

pub type EventId  = [u8; 32];
pub type PubKey   = [u8; 32];
pub type RelayUrl = String;
```

## Events

```rust
#[derive(Clone, Debug)]
pub struct StoredEvent {
    pub raw: Arc<nostr::Event>,     // upstream nostr crate type
    pub received_at_ms: u64,        // wall-clock first arrival across all relays
}
```

`StoredEvent::raw` is `Arc<nostr::Event>` so the hot LRU can hold reference-counted copies without cloning the event body on each `get_by_id`.

```rust
pub trait EventIter: Iterator<Item = Result<StoredEvent, StoreError>> + Send {}
impl<T: Iterator<Item = Result<StoredEvent, StoreError>> + Send> EventIter for T {}
```

Iterators are lazy so the planner can page results without materialising the full set.

## Insert outcomes

```rust
#[derive(Clone, Debug)]
pub enum InsertOutcome {
    /// Fresh insert; secondary indexes written.
    Inserted { id: EventId, sources_after: u32 },
    /// Duplicate id; provenance updated, primary untouched.
    Duplicate { id: EventId, sources_after: u32 },
    /// Replaceable supersession: this event replaced an older one.
    Replaced { new_id: EventId, replaced_id: EventId },
    /// Replaceable supersession: incoming was older, dropped.
    Superseded { id: EventId, current_id: EventId },
    /// Suppressed because a tombstone exists for this event id.
    /// `kind5_event_id` is None when the origin is NIP-40 expiry or admin purge,
    /// Some when a kind:5 triggered the tombstone.
    Tombstoned { id: EventId, kind5_event_id: Option<EventId>, origin: TombstoneOrigin },
    /// Signature / delegation / structural validity failed.
    Rejected { id: EventId, reason: RejectReason },
    /// Ephemeral kind: delivered to live consumers, not stored.
    Ephemeral { id: EventId },
}

#[derive(Clone, Debug)]
pub enum RejectReason {
    BadSignature,
    BadDelegation(String),
    Malformed(String),
    ExpiredOnArrival,               // NIP-40 expiration already in the past
}
```

## Tombstones

```rust
#[derive(Clone, Debug)]
pub struct TombstoneRow {
    pub target_id: EventId,
    pub kind5_event_id: Option<EventId>, // None for NIP-40 expiry and AdminPurge tombstones
    pub deleter_pubkey: Option<PubKey>,  // None for NIP40Expiry / AdminPurge
    pub deleted_at: u64,                 // unix seconds; max observed across redeliveries
    pub sources: Vec<RelayUrl>,
    pub origin: TombstoneOrigin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TombstoneOrigin { Kind5, NIP40Expiry, AdminPurge }
```

## Provenance

```rust
#[derive(Clone, Debug)]
pub struct ProvenanceEntry {
    pub relay_url: RelayUrl,
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
    pub primary: bool,              // first observed relay (deterministic; sources[0] after sort)
}
```

## Watermarks

```rust
#[derive(Clone, Debug)]
pub struct WatermarkKey {
    pub filter_hash: [u8; 32],
    pub relay_url: RelayUrl,
}

#[derive(Clone, Debug)]
pub struct WatermarkRow {
    pub key: WatermarkKey,
    pub synced_up_to: u64,          // unix seconds
    pub last_sync_method: SyncMethod,
    pub last_negentropy_state: Option<Vec<u8>>, // engine-opaque resume blob (M4)
    pub bytes_saved_vs_req: u64,
    pub updated_at: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncMethod { Negentropy, ReqScan, Manual }
```

## GC / hot-set

```rust
/// Opaque view-handle id assigned by the actor (monotonically increasing u64).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ClaimerId(pub u64);

/// Budget for one `gc_step()` call.
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
```

## Export

```rust
#[derive(Clone, Copy, Debug)]
pub enum DumpFormat { Jsonl, Cbor }

#[derive(Clone, Debug, Default)]
pub struct DumpStats {
    pub events: u64,
    pub tombstones: u64,
    pub watermarks: u64,
    pub domain_rows: u64,
    pub bytes_written: u64,
}
```

## Errors

```rust
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("backend i/o: {0}")] Io(String),
    #[error("backend corruption: {0}")] Corrupt(String),
    #[error("encoding: {0}")] Encoding(String),
    #[error("schema too new: {namespace} on-disk={on_disk} expected={expected}")]
    SchemaTooNew { namespace: String, on_disk: u32, expected: u32 },
    #[error("schema migration failed: {namespace} v{from}->v{to}: {reason}")]
    MigrationFailed { namespace: String, from: u32, to: u32, reason: String },
    #[error("unknown namespace: {0}")] UnknownNamespace(String),
    /// Returned by `claim()` when the per-view or global pinned ceiling is exceeded.
    /// The claim is rejected; no pin is written. The caller must release some
    /// existing claim or reduce the requested set before retrying.
    /// Maps to D8 (reactivity contract): a working-set overflow must be surfaced
    /// as a typed rejection, never silently tolerated (see ADR-0001..0004).
    #[error("claim ceiling exceeded: claimer={claimer:?} requested={requested} ceiling={ceiling}")]
    OverPinned { claimer: ClaimerId, requested: usize, ceiling: usize },
}
```

No `StoreError` ever crosses FFI (D6). The actor maps every variant to a typed `Effect` or a graceful degraded state. See [`../trait.md`](../trait.md) §5 for the full mapping.
