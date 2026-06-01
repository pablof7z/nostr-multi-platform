// Copyright (c) 2022-2023 Yuki Kishimoto
// Copyright (c) 2023-2025 Rust Nostr Developers
// Distributed under the MIT software license

//! LMDB storage backend for nostr apps — NMP fork of `nostr-lmdb` v0.44.1.
//!
//! Original upstream: `rust-nostr/nostr/database/nostr-lmdb`.
//! Fork rationale: see `crates/nmp-nostr-lmdb/upstream-source.txt`.
//!
//! Public NMP entry points (env-injection seam — the reason this fork exists):
//! * [`Lmdb::with_env`] — build the LMDB index layer against a caller-owned
//!   `heed::Env`. NMP's `LmdbEventStore` uses this so its own sub-dbs
//!   (watermarks, claims, provenance, domain rows) share one transaction
//!   with event writes.
//! * [`Lmdb::save_event_with_txn`] — the txn-scoped write primitive that
//!   already contains NIP-09 / replaceable / addressable / coordinate
//!   tombstone policy upstream. Promoted from `pub(crate)` to `pub` so
//!   NMP can call it directly inside its own `RwTxn`.
//! * [`Lmdb::env`] — accessor exposing the shared env for NMP-side
//!   sub-db creation.
//!
//! Upstream-compatible entry points kept intact for re-sync ergonomics:
//! [`NostrLMDB::open`], [`NostrLMDB::builder`]. NMP does not use these.
//!
//! ## D10 provenance contract (caller responsibility)
//!
//! This crate is the upstream-shape Nostr **event store**. By design it
//! knows nothing about relay URLs: [`Lmdb::save_event_with_txn`] and
//! [`Lmdb::store`] persist only the wire event and its indexes. Doctrine
//! D10 ("every stored event carries its source relay URL") is therefore
//! **not enforced at this layer** — it cannot be, without diverging the
//! fork from upstream.
//!
//! Instead, D10 is enforced by the env-injection caller (NMP's
//! `LmdbEventStore`): it must write the provenance row to its own
//! sub-db *inside the same `RwTxn`* that calls `save_event_with_txn`,
//! so the event and its provenance commit atomically (ADR-0011/0012).
//! Any NMP-side caller that drives the `save_event_with_txn` / `store`
//! seam without also writing provenance in that txn violates D10. Keep
//! that pairing in the calling crate, never here.

// NMP fork: `missing_docs` downgraded from `warn` to `allow` because the
// fork promotes a number of previously-`pub(crate)` methods to `pub`
// without authoring fresh docs — these are internal storage primitives,
// not a public API. Documentation work tracked as a Gate A follow-up.
#![allow(missing_docs)]
#![warn(rustdoc::bare_urls)]
#![allow(clippy::mutable_key_type)]
// Storage backend: every function returns the same `Error` type; `# Errors`
// docs would be pure repetition with no diagnostic value.
#![allow(clippy::missing_errors_doc)]
// Upstream fork: index key builder locals are intentionally named atc/ktc/kc/ac
// to mirror the index names — renaming would harm re-sync ergonomics.
#![allow(clippy::similar_names)]
// Upstream fork: field names *_index on index-key structs are the upstream convention.
#![allow(clippy::struct_field_names)]
// Upstream fork: DatabaseFilter is taken by value in 8 filter-dispatch methods
// (upstream API shape). Changing all callers would diverge from upstream.
#![allow(clippy::needless_pass_by_value)]

use std::path::{Path, PathBuf};

use nostr_database::prelude::*;

mod replaceable_freshness;
mod store;

pub use self::replaceable_freshness::{
    decode_timestamp, encode_timestamp, is_parameterized_replaceable, is_replaceable,
    ReplaceableCache, ReplaceableKey,
};
use self::store::Store;

// NMP-fork re-exports — see upstream-source.txt §"Surgical changes".
pub use self::store::error::Error as StoreError;
pub use self::store::lmdb::Lmdb;
pub use self::store::SaveEventStatus;

// 64-bit
#[cfg(target_pointer_width = "64")]
const MAP_SIZE: usize = 1024 * 1024 * 1024 * 32; // 32GB

// 32-bit
#[cfg(target_pointer_width = "32")]
const MAP_SIZE: usize = 0xFFFFF000; // 4GB (2^32-4096)

/// Nostr LMDB database builder
#[derive(Debug, Clone)]
pub struct NostrLmdbBuilder {
    /// Database path
    pub path: PathBuf,
    /// Custom map size
    ///
    /// By default, the following map size is used:
    /// - 32GB for 64-bit arch
    /// - 4GB for 32-bit arch
    pub map_size: Option<usize>,
    /// Maximum number of reader threads
    ///
    /// Defaults to 126 if not set
    pub max_readers: Option<u32>,
    /// Number of additional databases to allocate beyond the 9 internal ones
    ///
    /// Defaults to 0 if not set
    pub additional_dbs: Option<u32>,
}

impl NostrLmdbBuilder {
    /// New `LMDb` builder
    #[must_use]
    pub fn new<P>(path: P) -> Self
    where
        P: AsRef<Path>,
    {
        Self {
            path: path.as_ref().to_path_buf(),
            map_size: None,
            max_readers: None,
            additional_dbs: None,
        }
    }

    /// Map size
    ///
    /// By default, the following map size is used:
    /// - 32GB for 64-bit arch
    /// - 4GB for 32-bit arch
    #[must_use] 
    pub fn map_size(mut self, map_size: usize) -> Self {
        self.map_size = Some(map_size);
        self
    }

    /// Maximum number of reader threads
    ///
    /// Defaults to 126 if not set
    #[must_use] 
    pub fn max_readers(mut self, max_readers: u32) -> Self {
        self.max_readers = Some(max_readers);
        self
    }

    /// Number of additional databases to allocate beyond the 9 internal ones
    ///
    /// Defaults to 0 if not set
    #[must_use] 
    pub fn additional_dbs(mut self, additional_dbs: u32) -> Self {
        self.additional_dbs = Some(additional_dbs);
        self
    }

    /// Build
    #[must_use]
    pub fn build(self) -> Result<NostrLMDB, DatabaseError> {
        let map_size: usize = self.map_size.unwrap_or(MAP_SIZE);
        let max_readers: u32 = self.max_readers.unwrap_or(126);
        let additional_dbs: u32 = self.additional_dbs.unwrap_or(0);
        let db: Store = Store::open(self.path, map_size, max_readers, additional_dbs)
            .map_err(DatabaseError::backend)?;
        Ok(NostrLMDB { db })
    }
}

/// LMDB Nostr Database
#[derive(Debug)]
pub struct NostrLMDB {
    db: Store,
}

impl NostrLMDB {
    /// Open LMDB database
    #[must_use]
    #[inline]
    pub fn open<P>(path: P) -> Result<Self, DatabaseError>
    where
        P: AsRef<Path>,
    {
        Self::builder(path).build()
    }

    /// Get a new builder
    #[inline]
    pub fn builder<P>(path: P) -> NostrLmdbBuilder
    where
        P: AsRef<Path>,
    {
        NostrLmdbBuilder::new(path)
    }
}

impl NostrDatabase for NostrLMDB {
    #[inline]
    fn backend(&self) -> Backend {
        Backend::LMDB
    }

    fn save_event<'a>(
        &'a self,
        event: &'a Event,
    ) -> BoxedFuture<'a, Result<SaveEventStatus, DatabaseError>> {
        Box::pin(async move {
            self.db
                .save_event(event)
                .await
                .map_err(DatabaseError::backend)
        })
    }

    fn check_id<'a>(
        &'a self,
        event_id: &'a EventId,
    ) -> BoxedFuture<'a, Result<DatabaseEventStatus, DatabaseError>> {
        Box::pin(async move {
            if self
                .db
                .event_is_deleted(event_id)
                .map_err(DatabaseError::backend)?
            {
                Ok(DatabaseEventStatus::Deleted)
            } else if self
                .db
                .has_event(event_id)
                .map_err(DatabaseError::backend)?
            {
                Ok(DatabaseEventStatus::Saved)
            } else {
                Ok(DatabaseEventStatus::NotExistent)
            }
        })
    }

    fn event_by_id<'a>(
        &'a self,
        event_id: &'a EventId,
    ) -> BoxedFuture<'a, Result<Option<Event>, DatabaseError>> {
        Box::pin(async move {
            self.db
                .get_event_by_id(event_id)
                .map_err(DatabaseError::backend)
        })
    }

    fn count(&self, filter: Filter) -> BoxedFuture<'_, Result<usize, DatabaseError>> {
        Box::pin(async move { self.db.count(filter).map_err(DatabaseError::backend) })
    }

    fn query(&self, filter: Filter) -> BoxedFuture<'_, Result<Events, DatabaseError>> {
        Box::pin(async move { self.db.query(filter).map_err(DatabaseError::backend) })
    }

    fn negentropy_items(
        &self,
        filter: Filter,
    ) -> BoxedFuture<'_, Result<Vec<(EventId, Timestamp)>, DatabaseError>> {
        Box::pin(async move {
            self.db
                .negentropy_items(filter)
                .map_err(DatabaseError::backend)
        })
    }

    fn delete(&self, filter: Filter) -> BoxedFuture<'_, Result<(), DatabaseError>> {
        Box::pin(async move { self.db.delete(filter).await.map_err(DatabaseError::backend) })
    }

    #[inline]
    fn wipe(&self) -> BoxedFuture<'_, Result<(), DatabaseError>> {
        Box::pin(async move { self.db.wipe().await.map_err(DatabaseError::backend) })
    }
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
