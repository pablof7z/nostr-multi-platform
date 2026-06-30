//! Freshness-row deletion methods for `Lmdb`, extracted from `mod.rs` to
//! keep that file under the LOC ceiling. Child module can reach `Lmdb`'s
//! private fields.

use heed::RwTxn;

use super::super::error::Error;
use super::Lmdb;

impl Lmdb {
    /// F-TTL: Delete the freshness row for `key` within `txn` and eagerly
    /// evict it from the in-memory cache.
    ///
    /// Called on every deletion path that removes a replaceable/addressable
    /// event so stale TTL cache entries cannot cause a fresh re-fetch to be
    /// wrongly skipped or an older event to resurface.
    ///
    /// **Cache eviction is eager** (happens before the txn commits).  If the
    /// txn is later aborted, the LMDB row survives but the cache entry is
    /// gone.  On the next read, `get_check_again_after` will return `None` —
    /// a cache miss — which causes the caller to re-verify eagerly.  That is
    /// correct-but-eager, never wrong: the cache can only turn a re-verify
    /// into a skip; missing an entry cannot.  This is the safe direction for
    /// eviction.
    pub fn delete_freshness(
        &self,
        txn: &mut RwTxn,
        key: &crate::ReplaceableKey,
    ) -> Result<(), Error> {
        let lmdb_key = key.to_lmdb_key();
        self.replaceable_freshness.delete(txn, &lmdb_key)?;
        // Eagerly evict from cache — safe to do before commit (see doc).
        if let Ok(mut cache) = self.replaceable_freshness_cache.lock() {
            cache.remove(key);
        }
        Ok(())
    }

    /// F-TTL: Durably remove the next-check timestamp for a replaceable identity.
    ///
    /// Opens its own write transaction, removes the LMDB row, commits, and
    /// only then evicts the key from the in-memory cache.  A poisoned lock
    /// degrades to "cache miss next read" (a re-verify), which is correct-but-
    /// eager, never wrong.
    pub fn delete_freshness_committed(&self, key: &crate::ReplaceableKey) -> Result<(), Error> {
        let mut txn = self.write_txn()?;
        self.delete_freshness(&mut txn, key)?;
        txn.commit()?;
        if let Ok(mut cache) = self.replaceable_freshness_cache.lock() {
            cache.remove(key);
        }
        Ok(())
    }
}
