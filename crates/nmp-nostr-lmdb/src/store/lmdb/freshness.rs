// Copyright (c) 2024 Michael Dilger
// Copyright (c) 2022-2023 Yuki Kishimoto
// Copyright (c) 2023-2025 Rust Nostr Developers
// Distributed under the MIT software license

//! F-TTL replaceable freshness tracking and store anomaly counters.

use std::sync::atomic::Ordering as AtomicOrdering;

use heed::RwTxn;

use super::super::error::Error;
use super::{Lmdb, StoreAnomalySnapshot};

impl Lmdb {
    /// F-TTL: Get the next-check timestamp (unix milliseconds) for a replaceable event.
    ///
    /// Returns the `check_again_after` timestamp from the in-memory cache. If not found,
    /// returns `None` — the event has never been freshed-checked.
    #[must_use]
    pub fn get_check_again_after(&self, key: &crate::ReplaceableKey) -> Option<u64> {
        self.replaceable_freshness_cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(key).copied())
    }

    /// F-TTL: Write the next-check timestamp into the LMDB sub-db *within the
    /// caller-provided transaction only*.
    ///
    /// This does **not** touch the in-memory cache: a transaction that is later
    /// aborted must not leave the cache claiming a timestamp the durable store
    /// never recorded (cache/DB divergence). Callers that own the transaction
    /// lifecycle and want the cache updated must do so themselves *after* a
    /// successful `commit()` — or simply use
    /// [`set_check_again_after_committed`](Self::set_check_again_after_committed),
    /// which opens, commits, and updates the cache atomically.
    pub fn set_check_again_after(
        &self,
        key: &crate::ReplaceableKey,
        ts_ms: u64,
        txn: &mut RwTxn,
    ) -> Result<(), Error> {
        let lmdb_key = key.to_lmdb_key();
        let lmdb_value = crate::encode_timestamp(ts_ms);
        self.replaceable_freshness.put(txn, &lmdb_key, &lmdb_value)?;
        Ok(())
    }

    /// F-TTL: Durably stamp the next-check timestamp for a replaceable identity.
    ///
    /// Opens its own write transaction, writes the LMDB row, commits, and only
    /// then updates the in-memory cache. The cache is updated *after* the commit
    /// succeeds so an aborted/failed transaction can never leave the cache and
    /// the durable store disagreeing.
    ///
    /// This is the entry point used by the `EventStore` trait override, which
    /// cannot thread a `RwTxn` across the `Arc<dyn EventStore>` boundary.
    pub fn set_check_again_after_committed(
        &self,
        key: crate::ReplaceableKey,
        ts_ms: u64,
    ) -> Result<(), Error> {
        let mut txn = self.write_txn()?;
        self.set_check_again_after(&key, ts_ms, &mut txn)?;
        txn.commit()?;

        // Commit succeeded — now it is safe to reflect the value in the cache.
        // A poisoned lock degrades to "cache miss next read" (a re-verify),
        // which is correct-but-eager, never wrong.
        if let Ok(mut cache) = self.replaceable_freshness_cache.lock() {
            cache.insert(key, ts_ms);
        }

        Ok(())
    }

    /// NMP fork (V-69): snapshot of store-anomaly counters.
    ///
    /// A [`StoreAnomalySnapshot`] with all-zero fields means no index corruption
    /// has been detected since this `Lmdb` instance was opened. Tests and hosts
    /// can assert `store_anomaly_snapshot() == StoreAnomalySnapshot::default()`
    /// to confirm "no corruption detected".
    ///
    /// The counter values are read with `Relaxed` ordering — they are
    /// diagnostic only, not a synchronisation point.
    #[must_use]
    pub fn store_anomaly_snapshot(&self) -> StoreAnomalySnapshot {
        StoreAnomalySnapshot {
            orphan_index_entries: self
                .anomaly_orphan_index_entries
                .load(AtomicOrdering::Relaxed),
            unresolvable_events: self
                .anomaly_unresolvable_events
                .load(AtomicOrdering::Relaxed),
        }
    }
}
