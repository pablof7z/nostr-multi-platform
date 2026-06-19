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

/// Production map size: 32 GB on 64-bit.  Referenced by the error classifier
/// so that `ReaderExhaustion`/`MapFull` diagnostics carry the exact limit used.
pub(super) const MAP_SIZE: usize = 1024 * 1024 * 1024 * 32;

/// Maximum concurrent LMDB readers in the production configuration.
pub(super) const MAX_READERS: u32 = 126;

/// Open or create an LMDB store at `path`.
///
/// Shared-env design: `Lmdb::with_env` opens the upstream 11 sub-dbs on the
/// provided `Env`; we create 11 additional NMP sub-dbs on the same transaction
/// so all writes are atomic.
pub fn open_impl(path: &Path) -> Result<LmdbEventStore, StoreError> {
    open_impl_with_limits(path, MAP_SIZE, MAX_READERS)
}

/// Test-only seam: open with custom map_size / max_readers so that tests can
/// trigger `MapFull` / `ReaderExhaustion` without large allocations or long
/// waits.  Production code always calls `open_impl` which uses the constants.
pub(super) fn open_impl_with_limits(
    path: &Path,
    map_size: usize,
    max_readers: u32,
) -> Result<LmdbEventStore, StoreError> {
    use super::open_error::{classify_heed_err, classify_store_err};
    // NMP sub-dbs: provenance, tombstones, addr-tombstones,
    // domain-versions, domain-data, relay-author-scores, lru-access (V-60),
    // expiry-index (V-118), relay-index (V-52), coverage (K3 Stage D1),
    // relay-kind (#1518), interaction-counters (#1519).  The nmp-claims /
    // nmp-claims-budget sub-dbs were removed in #1090 Stage 1 (persisted claims
    // deleted in favour of a kernel-derived ephemeral pin set passed to
    // `gc_step_with_pins`).  The nmp-watermarks sub-db was removed in #1090
    // Stage 3 (dead persisted-watermark machinery had zero production callers);
    // the K3 coverage ledger below is its purpose-built, actually-read successor
    // (ADR-0056 §2.1 / §3 — re-created, not re-activated).
    const NMP_ADDITIONAL_DBS: u32 = 14;

    std::fs::create_dir_all(path).map_err(|e| StoreError::Io(e.to_string()))?;

    let env = Lmdb::open_env(path, map_size, max_readers, NMP_ADDITIONAL_DBS)
        .map_err(|e| classify_store_err(e, map_size, max_readers))?;
    let lmdb = Lmdb::with_env(env.clone())
        .map_err(|e| classify_store_err(e, map_size, max_readers))?;

    // Open NMP sub-dbs on the shared env in one write txn (atomic with the
    // upstream schema). The local closure keeps the call sites DRY.
    let mut txn = env
        .write_txn()
        .map_err(|e| classify_heed_err(e, map_size, max_readers))?;
    let open =
        |name: &str, txn: &mut heed::RwTxn| -> Result<heed::Database<Bytes, Bytes>, StoreError> {
            env.database_options()
                .types::<Bytes, Bytes>()
                .name(name)
                .create(txn)
                .map_err(|e| classify_heed_err(e, map_size, max_readers))
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
    // V-52 — relay-origin reverse index: relay_url || 0x00 || event_id(32) → empty.
    let relay_index = open("nmp-relay-index", &mut txn)?;
    // #1518 — relay×kind index: relay_url || 0x00 || kind(BE4) || event_id(32) → empty.
    let relay_kind = open("nmp-relay-kind", &mut txn)?;
    // K3 Stage D1 (ADR-0056 §3) — coverage ledger:
    // filter_hash || 0x1F || relay_url → covered_through(8 BE).
    let coverage = open("nmp-coverage", &mut txn)?;
    // Issue #1519 — interaction-counter sidecar.
    let interaction_counters = open("nmp-interaction-counters", &mut txn)?;
    // ADR-0058 §4 — ingest-log sub-dbs.
    let ingest_log_db = open("nmp-ingest-log", &mut txn)?;
    let ingest_meta_db = open("nmp-ingest-meta", &mut txn)?;

    // Initialise the in-memory seq counter from the max persisted value so
    // a crash-restart never reuses sequence numbers.
    let lru_seq_init: u64 = {
        let mut max_seq: u64 = 0;
        for entry in lru_access
            .iter(&txn)
            .map_err(|e| classify_heed_err(e, map_size, max_readers))?
        {
            let (_, v) =
                entry.map_err(|e| classify_heed_err(e, map_size, max_readers))?;
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
        .map_err(|e| classify_heed_err(e, map_size, max_readers))?;

    // V-118 — one-time backfill: populate the expiry index for any events that
    // were stored before the index existed (pre-V-118 databases).  Gated by
    // the domain_versions key so the O(store) scan runs exactly once.
    backfill_expiry_index(&env, &lmdb, expiry_index, domain_versions)?;

    // V-52 — one-time backfill: populate the relay index from existing
    // provenance for any events stored before the index existed (pre-V-52
    // databases).  Gated by the domain_versions key so the O(provenance) scan
    // runs exactly once.
    backfill_relay_index(&env, provenance, relay_index, domain_versions)?;

    // #1518 — one-time backfill: populate the relay×kind index from existing
    // events + provenance for any events stored before the index existed
    // (pre-#1518 databases).  Gated by the domain_versions key so the scan runs
    // exactly once.  Must run AFTER the relay-index backfill is irrelevant to
    // ordering — it reads provenance + events independently.
    backfill_relay_kind_index(
        &env,
        &lmdb,
        provenance,
        relay_kind,
        domain_versions,
        map_size,
        max_readers,
    )?;

    // Issue #1519 — interaction-counter schema init.
    let interaction_counters_usable =
        super::interaction_counters::init_schema(&env, domain_versions)?;

    Ok(LmdbEventStore {
        path: path.to_path_buf(),
        inner: Arc::new(Inner {
            env,
            lmdb,
            map_size,
            max_readers,
            provenance,
            tombstones,
            addr_tombstones,
            domain_versions,
            domain_data,
            relay_author_scores,
            lru_access,
            lru_seq: AtomicU64::new(lru_seq_init),
            expiry_index,
            relay_index,
            relay_kind,
            coverage,
            interaction_counters,
            interaction_counters_usable,
            ingest_log: ingest_log_db,
            ingest_meta: ingest_meta_db,
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
    // Convenience: map any write error in this migration to MigrationFailed.
    let migration_err = |_e: heed::Error| StoreError::MigrationFailed {
        namespace: "nmp-expiry-index".into(),
        from: 0,
        to: 1,
        reason: "backfill write failed (lmdb-io)".into(),
    };

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
    let mut txn = env.write_txn().map_err(migration_err)?;
    for (id, exp) in entries {
        let mut key = [0u8; 40];
        key[..8].copy_from_slice(&exp.to_be_bytes());
        key[8..].copy_from_slice(&id);
        expiry_index
            .put(&mut txn, &key, &[])
            .map_err(migration_err)?;
    }
    // Mark migration as done — this is the gate key checked on subsequent opens.
    domain_versions
        .put(&mut txn, EXPIRY_INDEX_BACKFILL_KEY, &1u32.to_be_bytes())
        .map_err(migration_err)?;
    txn.commit().map_err(migration_err)?;
    Ok(())
}

/// Key recording that the V-52 relay-index backfill has completed for this
/// store.  Shares the `nmp-domain-versions` sub-db; relies on the same
/// repository-wide `nmp-` prefix reservation as `EXPIRY_INDEX_BACKFILL_KEY`
/// (no host namespace can begin with `nmp-`).
const RELAY_INDEX_BACKFILL_KEY: &[u8] = b"nmp-relay-index";

/// Populate the relay-origin reverse index from existing provenance for any
/// events stored before the index existed (pre-V-52 databases).
///
/// **Migration gate**: the `domain_versions` sub-db is checked first.  If
/// `RELAY_INDEX_BACKFILL_KEY` is already present this returns immediately — the
/// O(provenance) scan runs exactly once per physical database.
///
/// The relay index is derived purely from provenance: for every stored event
/// we record one `(relay_url, event_id)` entry per relay in its provenance
/// list.  This mirrors the `MemEventStore::relay_index` which is likewise a
/// projection of per-event provenance.
fn backfill_relay_index(
    env: &Env,
    provenance: Database<Bytes, Bytes>,
    relay_index: Database<Bytes, Bytes>,
    domain_versions: Database<Bytes, Bytes>,
) -> Result<(), StoreError> {
    // Convenience: map any write error in this migration to MigrationFailed.
    let migration_err = |_e: heed::Error| StoreError::MigrationFailed {
        namespace: "nmp-relay-index".into(),
        from: 0,
        to: 1,
        reason: "backfill write failed (lmdb-io)".into(),
    };

    // O(1) gate — skip the full provenance scan if the migration already ran.
    {
        let txn = env
            .read_txn()
            .map_err(|e| StoreError::Io(format!("relay backfill gate read_txn: {e}")))?;
        if domain_versions
            .get(&txn, RELAY_INDEX_BACKFILL_KEY)
            .map_err(|e| StoreError::Io(format!("relay backfill gate get: {e}")))?
            .is_some()
        {
            return Ok(());
        }
    }

    // One-time scan: collect (relay_url, event_id) for every provenance entry.
    let entries: Vec<(String, [u8; 32])> = {
        let txn = env
            .read_txn()
            .map_err(|e| StoreError::Io(format!("relay backfill read_txn: {e}")))?;
        let mut out: Vec<(String, [u8; 32])> = Vec::new();
        for entry in provenance
            .iter(&txn)
            .map_err(|e| StoreError::Io(format!("relay backfill prov iter: {e}")))?
        {
            let (k, v) =
                entry.map_err(|e| StoreError::Io(format!("relay backfill prov step: {e}")))?;
            if k.len() != 32 {
                continue;
            }
            let mut id = [0u8; 32];
            id.copy_from_slice(k);
            for relay_url in super::provenance::decode_relays(v)? {
                out.push((relay_url, id));
            }
        }
        out
    };

    // Write index entries + version key in a single atomic transaction.
    let mut txn = env.write_txn().map_err(migration_err)?;
    for (relay_url, id) in entries {
        let key = super::provenance::relay_index_key(&relay_url, &id);
        relay_index
            .put(&mut txn, &key, &[])
            .map_err(migration_err)?;
    }
    domain_versions
        .put(&mut txn, RELAY_INDEX_BACKFILL_KEY, &1u32.to_be_bytes())
        .map_err(migration_err)?;
    txn.commit().map_err(migration_err)?;
    Ok(())
}

/// Key recording that the #1518 relay×kind-index backfill has completed for this
/// store.  Shares the `nmp-domain-versions` sub-db; relies on the same
/// repository-wide `nmp-` prefix reservation as the other backfill keys.
const RELAY_KIND_BACKFILL_KEY: &[u8] = b"nmp-relay-kind";

/// Populate the relay×kind index from existing events + provenance for any
/// events stored before the index existed (pre-#1518 databases).
///
/// **Migration gate**: the `domain_versions` sub-db is checked first.  If
/// `RELAY_KIND_BACKFILL_KEY` is already present this returns immediately — the
/// scan runs exactly once per physical database.
///
/// The relay×kind index is derived from provenance keyed by the event's kind:
/// for every stored event we record one `(relay_url, kind, event_id)` entry per
/// relay in its provenance list, skipping privacy-gated kinds (checked inside
/// `provenance::relay_kind_put`).  The event's kind comes from the primary
/// store (one query over all events to build an `id → kind` map).
#[allow(clippy::too_many_arguments)]
fn backfill_relay_kind_index(
    env: &Env,
    lmdb: &Lmdb,
    provenance: Database<Bytes, Bytes>,
    relay_kind: Database<Bytes, Bytes>,
    domain_versions: Database<Bytes, Bytes>,
    map_size: usize,
    max_readers: u32,
) -> Result<(), StoreError> {
    // O(1) gate — skip the full scan if the migration already ran.
    {
        let txn = env
            .read_txn()
            .map_err(|e| StoreError::Io(format!("relay-kind backfill gate read_txn: {e}")))?;
        if domain_versions
            .get(&txn, RELAY_KIND_BACKFILL_KEY)
            .map_err(|e| StoreError::Io(format!("relay-kind backfill gate get: {e}")))?
            .is_some()
        {
            return Ok(());
        }
    }

    // One-time scan: build the id → kind map from the primary store, then walk
    // provenance to collect (relay_url, kind, event_id) for every entry.
    let entries: Vec<(String, u32, [u8; 32])> = {
        let txn = env
            .read_txn()
            .map_err(|e| StoreError::Io(format!("relay-kind backfill read_txn: {e}")))?;

        // id → kind from every stored event.
        let mut kind_for_id: std::collections::HashMap<[u8; 32], u32> =
            std::collections::HashMap::new();
        let iter = lmdb
            .query(&txn, nostr::Filter::new())
            .map_err(|e| StoreError::Io(format!("relay-kind backfill query: {e}")))?;
        for ev in iter {
            let owned: nostr::Event = ev.into_owned();
            let mut id = [0u8; 32];
            id.copy_from_slice(owned.id.as_bytes());
            kind_for_id.insert(id, owned.kind.as_u16() as u32);
        }

        let mut out: Vec<(String, u32, [u8; 32])> = Vec::new();
        for entry in provenance
            .iter(&txn)
            .map_err(|e| StoreError::Io(format!("relay-kind backfill prov iter: {e}")))?
        {
            let (k, v) =
                entry.map_err(|e| StoreError::Io(format!("relay-kind backfill prov step: {e}")))?;
            if k.len() != 32 {
                continue;
            }
            let mut id = [0u8; 32];
            id.copy_from_slice(k);
            // An event must still be present in the primary store to be indexed
            // — a provenance row with no event is a dangling artefact we skip.
            let Some(&kind) = kind_for_id.get(&id) else {
                continue;
            };
            for relay_url in super::provenance::decode_relays(v)? {
                out.push((relay_url, kind, id));
            }
        }
        out
    };

    // Write index entries + version key in a single atomic transaction.  The
    // privacy gate lives in `relay_kind_put` so backfill can never re-introduce
    // a privacy-gated kind.
    let mut txn = env
        .write_txn()
        .map_err(|e| StoreError::Io(format!("relay-kind backfill write_txn: {e}")))?;
    for (relay_url, kind, id) in entries {
        super::provenance::relay_kind_put(
            relay_kind, &mut txn, &relay_url, kind, &id, map_size, max_readers,
        )?;
    }
    domain_versions
        .put(&mut txn, RELAY_KIND_BACKFILL_KEY, &1u32.to_be_bytes())
        .map_err(|e| StoreError::Io(format!("relay-kind backfill version put: {e}")))?;
    txn.commit()
        .map_err(|e| StoreError::Io(format!("relay-kind backfill commit: {e}")))?;
    Ok(())
}
