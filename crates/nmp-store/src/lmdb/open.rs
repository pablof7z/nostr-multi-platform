//! LMDB environment + sub-db open logic (feature-on only).
//!
//! Extracted from `mod.rs` to keep that file under the 300-line soft ceiling.
//! The entry-point is `open_impl` — called by `LmdbEventStore::open`.

use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use heed::types::Bytes;
use heed::{Database, Env};
use nmp_nostr_lmdb::Lmdb;

use super::inner::Inner;
use super::relay_scores;
use super::LmdbEventStore;
use crate::StoreError;

/// Open or create an LMDB store at `path`.
///
/// Shared-env design: `Lmdb::with_env` opens the upstream 11 sub-dbs on the
/// provided `Env`; we create 11 additional NMP sub-dbs on the same transaction
/// so all writes are atomic.
pub fn open_impl(path: &Path) -> Result<LmdbEventStore, StoreError> {
    // 32 GB on 64-bit; the upstream default. The fork's `with_env` wraps the
    // 11 internal sub-dbs; we reserve 11 additional for NMP-side data.
    const MAP_SIZE: usize = 1024 * 1024 * 1024 * 32;
    const MAX_READERS: u32 = 126;
    // +1 for nmp-lru-access (V-60), +1 for nmp-expiry-index (V-118).
    const NMP_ADDITIONAL_DBS: u32 = 11;

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
    // V-118 — expiry index: expiry_ts(8 BE) || event_id(32) → empty.
    let expiry_index = open("nmp-expiry-index", &mut txn)?;

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

    // V-118 — one-time backfill: populate the expiry index for any events that
    // were stored before the index existed (pre-V-118 databases).  Idempotent
    // and cheap on a fresh store (the scan finds nothing to write).
    backfill_expiry_index(&env, &lmdb, expiry_index)?;

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
            expiry_index,
            gc_last_tombstone_purge_secs: AtomicU64::new(0),
        }),
    })
}

/// Populate the expiry index for any events already in the store that have an
/// expiration tag but are not yet indexed.
///
/// This is a one-time migration that runs every store open.  It is idempotent:
/// putting a key that already exists is a no-op in LMDB.  On a fresh store it
/// is a very cheap no-op scan; on a pre-V-118 store it iterates all events once
/// and writes one index entry per expiring event.
fn backfill_expiry_index(
    env: &Env,
    lmdb: &Lmdb,
    expiry_index: Database<Bytes, Bytes>,
) -> Result<(), StoreError> {
    // Collect (event_id, expiry_ts) for every event carrying an expiration tag.
    let entries: Vec<([u8; 32], u64)> = {
        let txn = env
            .read_txn()
            .map_err(|e| StoreError::Io(format!("backfill read_txn: {e}")))?;
        let iter = lmdb
            .query(&txn, nostr::Filter::new())
            .map_err(|e| StoreError::Io(format!("backfill query: {e}")))?;
        let mut out: Vec<([u8; 32], u64)> = Vec::new();
        for ev in iter {
            let owned: nostr::Event = ev.into_owned();
            if let Some(exp_tag) = owned
                .tags
                .iter()
                .find(|t| t.as_slice().first().map(|s| s == "expiration").unwrap_or(false))
            {
                if let Some(val) = exp_tag.as_slice().get(1) {
                    if let Ok(exp) = val.parse::<u64>() {
                        let mut id = [0u8; 32];
                        id.copy_from_slice(owned.id.as_bytes());
                        out.push((id, exp));
                    }
                }
            }
        }
        out
    };

    if entries.is_empty() {
        return Ok(());
    }

    let mut txn = env
        .write_txn()
        .map_err(|e| StoreError::Io(format!("backfill write_txn: {e}")))?;
    for (id, exp) in entries {
        let mut key = [0u8; 40];
        key[..8].copy_from_slice(&exp.to_be_bytes());
        key[8..].copy_from_slice(&id);
        expiry_index
            .put(&mut txn, &key, &[])
            .map_err(|e| StoreError::Io(format!("backfill put: {e}")))?;
    }
    txn.commit()
        .map_err(|e| StoreError::Io(format!("backfill commit: {e}")))?;
    Ok(())
}
