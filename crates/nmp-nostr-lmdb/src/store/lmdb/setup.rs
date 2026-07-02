// Copyright (c) 2024 Michael Dilger
// Copyright (c) 2022-2023 Yuki Kishimoto
// Copyright (c) 2023-2025 Rust Nostr Developers
// Distributed under the MIT software license

//! Database environment setup and constructor logic for the LMDB event store.

use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use super::super::error::Error;
use super::Lmdb;
use heed::byteorder::NativeEndian;
use heed::types::{Bytes, Unit, U64};
use heed::{Database, Env, EnvFlags, EnvOpenOptions};

impl Lmdb {
    /// Path-based constructor (upstream-compatible). Opens a fresh `heed::Env`
    /// at `path` and then opens this crate's databases on it. Equivalent
    /// behavior to upstream `Lmdb::new` pre-fork.
    ///
    /// NMP fork note: previously `pub(super)`. Kept available so the
    /// upstream `Store`/`NostrLMDB` builder path still works for re-sync
    /// checking, but NMP itself uses [`Lmdb::with_env`].
    pub(in crate::store) fn new<P>(
        path: P,
        map_size: usize,
        max_readers: u32,
        additional_dbs: u32,
    ) -> Result<Self, Error>
    where
        P: AsRef<Path>,
    {
        let env: Env = Self::open_env(path, map_size, max_readers, additional_dbs)?;
        Self::open_databases_on_env(env, true)
    }

    /// NMP fork: open just the `heed::Env` (no database initialization).
    /// Callers that want full ownership of the env — including the ability
    /// to open additional sub-dbs on it — use this then call
    /// [`Lmdb::with_env`].
    ///
    /// `additional_dbs` reserves sub-db slots beyond the 12 this crate uses
    /// internally, so NMP can open watermark / claim / provenance / domain
    /// sub-dbs without exhausting `max_dbs`.
    pub fn open_env<P: AsRef<Path>>(
        path: P,
        map_size: usize,
        max_readers: u32,
        additional_dbs: u32,
    ) -> Result<Env, Error> {
        let env: Env = unsafe {
            EnvOpenOptions::new()
                .flags(EnvFlags::NO_TLS)
                .max_dbs(12 + additional_dbs)
                .max_readers(max_readers)
                .map_size(map_size)
                .open(path)?
        };
        Ok(env)
    }

    /// NMP fork: open this crate's 12 databases on a caller-supplied env and
    /// (optionally) run migrations. Used by both `Lmdb::new` (path path) and
    /// `Lmdb::with_env` (env-injection path).
    pub(super) fn open_databases_on_env(env: Env, run_migrations: bool) -> Result<Self, Error> {
        // Acquire write transaction
        let mut txn = env.write_txn()?;

        // Open/Create maps
        let events: Database<Bytes, Bytes> = env
            .database_options()
            .types::<Bytes, Bytes>()
            .create(&mut txn)?;
        let ci_index: Database<Bytes, Bytes> = env
            .database_options()
            .types::<Bytes, Bytes>()
            .name("ci")
            .create(&mut txn)?;
        let tc_index: Database<Bytes, Bytes> = env
            .database_options()
            .types::<Bytes, Bytes>()
            .name("tci")
            .create(&mut txn)?;
        let ac_index: Database<Bytes, Bytes> = env
            .database_options()
            .types::<Bytes, Bytes>()
            .name("aci")
            .create(&mut txn)?;
        let akc_index: Database<Bytes, Bytes> = env
            .database_options()
            .types::<Bytes, Bytes>()
            .name("akci")
            .create(&mut txn)?;
        let atc_index: Database<Bytes, Bytes> = env
            .database_options()
            .types::<Bytes, Bytes>()
            .name("atci")
            .create(&mut txn)?;
        let kc_index: Database<Bytes, Bytes> = env
            .database_options()
            .types::<Bytes, Bytes>()
            .name("kci")
            .create(&mut txn)?;
        let ktc_index: Database<Bytes, Bytes> = env
            .database_options()
            .types::<Bytes, Bytes>()
            .name("ktci")
            .create(&mut txn)?;
        let deleted_ids: Database<Bytes, Unit> = env
            .database_options()
            .types::<Bytes, Unit>()
            .name("deleted-ids")
            .create(&mut txn)?;
        let deleted_coordinates: Database<Bytes, U64<NativeEndian>> = env
            .database_options()
            .types::<Bytes, U64<NativeEndian>>()
            .name("deleted-coordinates")
            .create(&mut txn)?;
        let metadata: Database<Bytes, U64<NativeEndian>> = env
            .database_options()
            .types::<Bytes, U64<NativeEndian>>()
            .name("metadata")
            .create(&mut txn)?;
        let replaceable_freshness: Database<Bytes, Bytes> = env
            .database_options()
            .types::<Bytes, Bytes>()
            .name("replaceable-freshness")
            .create(&mut txn)?;

        // Commit changes
        txn.commit()?;

        // Hot-load replaceable freshness cache from LMDB
        let mut cache = crate::ReplaceableCache::new();
        let rtxn = env.read_txn()?;
        for result in replaceable_freshness.iter(&rtxn)? {
            let (key_bytes, ts_bytes) = result?;
            // We need the kind to deserialize; extract it from the key prefix.
            // A row whose key is shorter than 4 bytes is corrupt — skip it
            // rather than panic (no `unwrap` on the hot-load production path).
            if let Some(kind_bytes) = key_bytes.get(..4) {
                if let Ok(kind_arr) = <[u8; 4]>::try_from(kind_bytes) {
                    let kind = u32::from_be_bytes(kind_arr);
                    if let Ok(k) = crate::ReplaceableKey::from_lmdb_key(key_bytes, kind) {
                        if let Ok(ts) = crate::decode_timestamp(ts_bytes) {
                            cache.insert(k, ts);
                        }
                    }
                }
            }
        }
        rtxn.commit()?;

        let lmdb = Self {
            env,
            events,
            ci_index,
            tc_index,
            ac_index,
            akc_index,
            atc_index,
            kc_index,
            ktc_index,
            deleted_ids,
            deleted_coordinates,
            metadata,
            replaceable_freshness,
            replaceable_freshness_cache: Arc::new(std::sync::Mutex::new(cache)),
            anomaly_orphan_index_entries: Arc::new(AtomicU64::new(0)),
            anomaly_unresolvable_events: Arc::new(AtomicU64::new(0)),
        };

        // Check and run migrations if needed
        if run_migrations {
            lmdb.migrate()?;
        }

        Ok(lmdb)
    }

    /// NMP fork: build the LMDB index layer against a caller-owned env.
    ///
    /// **The reason this fork exists.** ADR-0072 requires NMP to own the
    /// `heed::Env` so its sub-dbs (watermarks, claims, provenance, domain
    /// rows) commit atomically with event writes inside a single `RwTxn`.
    /// `with_env` lets the caller hand in a pre-opened env (typically via
    /// `Lmdb::open_env`, which reserves `additional_dbs` slots for the
    /// caller's own sub-dbs).
    ///
    /// Runs upstream migrations on the env before returning.
    #[must_use]
    pub fn with_env(env: Env) -> Result<Self, Error> {
        Self::open_databases_on_env(env, true)
    }

    /// NMP fork: accessor for the underlying `heed::Env` so callers can
    /// open their own sub-dbs on the same environment (atomicity
    /// guarantee). The env is `Clone`able under heed's semantics — clone
    /// it freely.
    #[must_use]
    pub fn env(&self) -> &Env {
        &self.env
    }
}
