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
    // 11 internal sub-dbs; we reserve 9 additional for NMP-side data.
    const MAP_SIZE: usize = 1024 * 1024 * 1024 * 32;
    const MAX_READERS: u32 = 126;
    // NMP sub-dbs: provenance, tombstones, addr-tombstones,
    // domain-versions, domain-data, relay-author-scores, lru-access (V-60),
    // expiry-index (V-118).  The nmp-claims / nmp-claims-budget sub-dbs were
    // removed in #1090 Stage 1 (persisted claims deleted in favour of a
    // kernel-derived ephemeral pin set passed to `gc_step_with_pins`).  The
    // nmp-watermarks sub-db was removed in #1090 Stage 3 (dead persisted-
    // watermark machinery had zero production callers).
    const NMP_ADDITIONAL_DBS: u32 = 8;

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
    // were stored before the index existed (pre-V-118 databases).  Gated by
    // the domain_versions key so the O(store) scan runs exactly once.
    backfill_expiry_index(&env, &lmdb, expiry_index, domain_versions)?;

    Ok(LmdbEventStore {
        path: path.to_path_buf(),
        inner: Arc::new(Inner {
            env,
            lmdb,
            provenance,
            tombstones,
            addr_tombstones,
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

/// Key used in the `domain_versions` sub-db to record that the V-118 expiry
/// index backfill has completed for this store.  Once this key is present the
/// O(store) scan is skipped on every subsequent open.
///
/// **Keyspace note**: this key intentionally shares the `nmp-domain-versions`
/// sub-db with the host-namespace version keys written by
/// `domain.rs::run_migrations`.  Those callers use arbitrary user-supplied
/// namespace strings as keys; this constant relies on the repository-wide
/// `nmp-` prefix reservation (all internal NMP sub-db names start with `nmp-`)
/// to guarantee no host namespace can collide with this value.
const EXPIRY_INDEX_BACKFILL_KEY: &[u8] = b"nmp-expiry-index";

/// Populate the expiry index for any events already in the store that have an
/// expiration tag but are not yet indexed.
///
/// **Migration gate**: the `domain_versions` sub-db is checked first.  If the
/// key `EXPIRY_INDEX_BACKFILL_KEY` is already present this function returns
/// immediately without touching the event store — the O(store) scan only runs
/// once per physical database.  After the scan completes the key is written so
/// subsequent opens skip straight through.
///
/// On a fresh store the scan finds nothing and the version key is written in a
/// single atomic transaction.  On a pre-V-118 store it iterates all events once
/// and writes one index entry per expiring event.
///
/// **Tag parsing**: uses `nostr::Tags::expiration()` (NIP-40 helper) rather than
/// hand-parsing the raw tag slice, which removes the duplicated parsing logic.
fn backfill_expiry_index(
    env: &Env,
    lmdb: &Lmdb,
    expiry_index: Database<Bytes, Bytes>,
    domain_versions: Database<Bytes, Bytes>,
) -> Result<(), StoreError> {
    // O(1) gate — skip the full-store scan if the migration already ran.
    {
        let txn = env
            .read_txn()
            .map_err(|e| StoreError::Io(format!("backfill gate read_txn: {e}")))?;
        if domain_versions
            .get(&txn, EXPIRY_INDEX_BACKFILL_KEY)
            .map_err(|e| StoreError::Io(format!("backfill gate get: {e}")))?
            .is_some()
        {
            return Ok(());
        }
    }

    // One-time scan: collect (event_id, expiry_ts) for events with an expiration tag.
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
            // Use the nostr crate's NIP-40 accessor rather than hand-parsing the tag slice.
            if let Some(exp) = owned.tags.expiration() {
                let mut id = [0u8; 32];
                id.copy_from_slice(owned.id.as_bytes());
                out.push((id, exp.as_secs()));
            }
        }
        out
    };

    // Write index entries + version key in a single atomic transaction.
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
    // Mark migration as done — this is the gate key checked on subsequent opens.
    domain_versions
        .put(&mut txn, EXPIRY_INDEX_BACKFILL_KEY, &1u32.to_be_bytes())
        .map_err(|e| StoreError::Io(format!("backfill version put: {e}")))?;
    txn.commit()
        .map_err(|e| StoreError::Io(format!("backfill commit: {e}")))?;
    Ok(())
}
