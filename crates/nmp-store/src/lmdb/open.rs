//! LMDB environment + sub-db open logic (feature-on only).
//!
//! Extracted from `mod.rs` to keep that file under the 300-line soft ceiling.
//! The entry-point is `open_impl` — called by `LmdbEventStore::open`.

use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use heed::types::Bytes;
use nmp_nostr_lmdb::Lmdb;

use super::inner::Inner;
use super::relay_scores;
use super::LmdbEventStore;
use crate::StoreError;

/// Open or create an LMDB store at `path`.
///
/// Shared-env design: `Lmdb::with_env` opens the upstream 11 sub-dbs on the
/// provided `Env`; we create 10 additional NMP sub-dbs on the same transaction
/// so all writes are atomic.
pub fn open_impl(path: &Path) -> Result<LmdbEventStore, StoreError> {
    // 32 GB on 64-bit; the upstream default. The fork's `with_env` wraps the
    // 11 internal sub-dbs; we reserve 10 additional for NMP-side data.
    const MAP_SIZE: usize = 1024 * 1024 * 1024 * 32;
    const MAX_READERS: u32 = 126;
    const NMP_ADDITIONAL_DBS: u32 = 10; // +1 for nmp-lru-access (V-60)

    std::fs::create_dir_all(path).map_err(|e| StoreError::Io(e.to_string()))?;

    let env = Lmdb::open_env(path, MAP_SIZE, MAX_READERS, NMP_ADDITIONAL_DBS)
        .map_err(|e| StoreError::Io(format!("open_env: {e}")))?;
    let lmdb = Lmdb::with_env(env.clone()).map_err(|e| StoreError::Io(format!("with_env: {e}")))?;

    // Open NMP sub-dbs on the shared env in one write txn (atomic with the
    // upstream schema). The local closure keeps the call sites DRY.
    let mut txn = env
        .write_txn()
        .map_err(|e| StoreError::Io(format!("write_txn: {e}")))?;
    let open =
        |name: &str, txn: &mut heed::RwTxn| -> Result<heed::Database<Bytes, Bytes>, StoreError> {
            env.database_options()
                .types::<Bytes, Bytes>()
                .name(name)
                .create(txn)
                .map_err(|e| StoreError::Io(format!("open {name}: {e}")))
        };
    let provenance = open("nmp-provenance", &mut txn)?;
    let tombstones = open("nmp-tombstones", &mut txn)?;
    let addr_tombstones = open("nmp-addr-tombstones", &mut txn)?;
    let watermarks = open("nmp-watermarks", &mut txn)?;
    let claims_budget = open("nmp-claims-budget", &mut txn)?;
    let claims = open("nmp-claims", &mut txn)?;
    let domain_versions = open("nmp-domain-versions", &mut txn)?;
    let domain_data = open("nmp-domain-data", &mut txn)?;
    // W2 — relay-author-scores sub-db.
    let relay_author_scores = open(relay_scores::SUB_DB_NAME, &mut txn)?;
    // V-60 — LRU access index: event_id(32) → seq(8 BE).
    let lru_access = open("nmp-lru-access", &mut txn)?;

    // Initialise the in-memory seq counter from the max persisted value so
    // a crash-restart never reuses sequence numbers.
    let lru_seq_init: u64 = {
        let mut max_seq: u64 = 0;
        for entry in lru_access
            .iter(&txn)
            .map_err(|e| StoreError::Io(format!("lru iter: {e}")))?
        {
            let (_, v) = entry.map_err(|e| StoreError::Io(format!("lru entry: {e}")))?;
            if v.len() >= 8 {
                let seq = u64::from_be_bytes(v[..8].try_into().unwrap());
                if seq > max_seq {
                    max_seq = seq;
                }
            }
        }
        max_seq
    };

    txn.commit()
        .map_err(|e| StoreError::Io(format!("commit init: {e}")))?;

    Ok(LmdbEventStore {
        path: path.to_path_buf(),
        inner: Arc::new(Inner {
            env,
            lmdb,
            provenance,
            tombstones,
            addr_tombstones,
            watermarks,
            claims_budget,
            claims,
            domain_versions,
            domain_data,
            relay_author_scores,
            lru_access,
            lru_seq: AtomicU64::new(lru_seq_init),
        }),
    })
}
