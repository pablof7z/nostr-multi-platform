// Copyright (c) 2024 Michael Dilger
// Copyright (c) 2022-2023 Yuki Kishimoto
// Copyright (c) 2023-2025 Rust Nostr Developers
// Distributed under the MIT software license

use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use heed::byteorder::NativeEndian;
use heed::types::{Bytes, Unit, U64};
use heed::{Database, Env, RoTxn, RwTxn};

use super::filter::DatabaseFilter;

mod cursor;
mod freshness;
mod freshness_delete;
mod index;
mod migration;
mod query;
mod setup;
mod write;

#[cfg(test)]
mod tests;

use super::error::Error;

pub(super) const EVENT_ID_ALL_ZEROS: [u8; 32] = [0; 32];
pub(super) const EVENT_ID_ALL_255: [u8; 32] = [255; 32];

/// Current database schema version
pub(super) const DB_VERSION: u64 = 2;
pub(super) const DB_VERSION_KEY: &[u8] = b"db_version";

/// A snapshot of store-anomaly counters for diagnostic purposes.
///
/// A non-zero field indicates index corruption was detected during queries.
/// Tests and hosts can assert all fields are zero to confirm "no corruption
/// detected". Counters are monotonically increasing — they never reset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StoreAnomalySnapshot {
    /// Number of ci_index entries that pointed to a missing event (dangling
    /// pointer). Incremented by [`Lmdb::query_by_scraping`] whenever
    /// [`Lmdb::get_event_by_id`] returns `Ok(None)` for an index value.
    pub orphan_index_entries: u64,
    /// Number of ci_index entries for which event deserialization failed
    /// (FlatBuffers decode error or LMDB I/O error on the event row itself).
    /// Distinct from `orphan_index_entries` because the event row exists but
    /// cannot be read — this usually indicates on-disk corruption rather than
    /// a dangling index pointer.
    pub unresolvable_events: u64,
}

#[derive(Debug)]
pub(super) enum QueryFilterPattern {
    Ids,
    AuthorsAndKinds,
    AuthorsAndTags,
    AuthorKindsAndTags,
    KindsAndTags,
    Tags,
    Authors,
    Kinds,
    Scraping,
}

impl QueryFilterPattern {
    pub(super) fn from_filter(filter: &DatabaseFilter) -> Self {
        if !filter.ids.is_empty() {
            Self::Ids
        } else if !filter.authors.is_empty()
            && !filter.kinds.is_empty()
            && !filter.generic_tags.is_empty()
        {
            Self::AuthorKindsAndTags
        } else if !filter.authors.is_empty() && !filter.kinds.is_empty() {
            Self::AuthorsAndKinds
        } else if !filter.authors.is_empty() && !filter.generic_tags.is_empty() {
            Self::AuthorsAndTags
        } else if !filter.kinds.is_empty() && !filter.generic_tags.is_empty() {
            Self::KindsAndTags
        } else if !filter.generic_tags.is_empty() {
            Self::Tags
        } else if !filter.authors.is_empty() {
            Self::Authors
        } else if !filter.kinds.is_empty() {
            Self::Kinds
        } else {
            Self::Scraping
        }
    }
}

// NMP fork: `Lmdb` promoted from `pub(crate)` to `pub` so the crate root
// can re-export it as the primary entry point for env-injection callers.
#[derive(Debug, Clone)]
pub struct Lmdb {
    /// LMDB env
    pub(super) env: Env,
    /// Events
    pub(super) events: Database<Bytes, Bytes>, // Event ID, Event
    /// `CreatedAt` + ID index
    pub(super) ci_index: Database<Bytes, Bytes>, // <Index>, Event ID
    /// Tag + `CreatedAt` + ID index
    pub(super) tc_index: Database<Bytes, Bytes>, // <Index>, Event ID
    /// Author + `CreatedAt` + ID index
    pub(super) ac_index: Database<Bytes, Bytes>, // <Index>, Event ID
    /// Author + Kind + `CreatedAt` + ID index
    pub(super) akc_index: Database<Bytes, Bytes>, // <Index>, Event ID
    /// Author + Tag + `CreatedAt` + ID index
    pub(super) atc_index: Database<Bytes, Bytes>, // <Index>, Event ID
    /// Kind + `CreatedAt` + ID index
    pub(super) kc_index: Database<Bytes, Bytes>, // <Index>, Event ID
    /// Kind + Tag + `CreatedAt` + ID index
    pub(super) ktc_index: Database<Bytes, Bytes>, // <Index>, Event ID
    /// Deleted IDs
    pub(super) deleted_ids: Database<Bytes, Unit>, // Event ID
    /// Deleted coordinates
    pub(super) deleted_coordinates: Database<Bytes, U64<NativeEndian>>, // Coordinate, UNIX timestamp
    /// Database metadata (version, etc)
    pub(super) metadata: Database<Bytes, U64<NativeEndian>>, // Key, Value
    /// F-TTL replaceable freshness: kind[4B BE]||pubkey[32B]||d_tag_utf8[var] → check_again_after_unix_ms[8B BE]
    pub(super) replaceable_freshness: Database<Bytes, Bytes>,
    /// In-memory cache of replaceable freshness (populated on open, read from cache, written to LMDB).
    /// Protected by Mutex because Lmdb is Clone and may be used from multiple threads.
    pub(super) replaceable_freshness_cache: Arc<std::sync::Mutex<crate::ReplaceableCache>>,
    /// Monotonic counter: ci_index entries whose event row was missing (V-69).
    /// Shared across all clones via Arc so any query path updates the same counter.
    pub(super) anomaly_orphan_index_entries: Arc<AtomicU64>,
    /// Monotonic counter: ci_index entries whose event row existed but could
    /// not be deserialized (V-69).
    pub(super) anomaly_unresolvable_events: Arc<AtomicU64>,
}

impl Lmdb {
    /// Get a read transaction
    ///
    /// This should never block the current thread
    #[must_use]
    #[inline]
    pub fn read_txn(&self) -> Result<RoTxn<'_>, Error> {
        Ok(self.env.read_txn()?)
    }

    /// Get a write transaction
    ///
    /// This blocks the current thread if there is another write txn
    #[must_use]
    #[inline]
    pub fn write_txn(&self) -> Result<RwTxn<'_>, Error> {
        Ok(self.env.write_txn()?)
    }
}
