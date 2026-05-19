//! NMP fork acceptance test: env-injection seam exposed by [`Lmdb::with_env`].
//!
//! Proves the four shape claims that justify carrying this fork:
//!
//! 1. `Lmdb::open_env` returns a caller-owned `heed::Env`.
//! 2. `Lmdb::with_env` wraps that env and runs upstream migrations.
//! 3. `Lmdb::save_event_with_txn` is reachable from a downstream caller
//!    and accepts a caller-managed `RwTxn`.
//! 4. The same `RwTxn` can be used to open additional sub-dbs on the
//!    NMP side and commit them atomically with the event write.
//!
//! These are the only contracts NMP's `LmdbEventStore` relies on
//! (`docs/decisions/0011-lmdb-env-sharing.md`).

use heed::types::Bytes;
use nmp_nostr_lmdb::{Lmdb, SaveEventStatus};
use nostr::prelude::*;
use nostr_database::FlatBufferBuilder;

/// Same map size we use in tests — keep well below the 32 GB default so
/// the OS does not pre-reserve large virtual address ranges.
const TEST_MAP_SIZE: usize = 1024 * 1024 * 100; // 100 MB
const TEST_MAX_READERS: u32 = 16;
const TEST_ADDITIONAL_DBS: u32 = 4; // NMP-side sub-dbs reserved here.

#[test]
fn open_env_then_with_env_round_trips_a_signed_event() {
    let tmp = tempfile::tempdir().expect("tempdir");

    // (1) Open the env directly — caller owns it.
    let env = Lmdb::open_env(tmp.path(), TEST_MAP_SIZE, TEST_MAX_READERS, TEST_ADDITIONAL_DBS)
        .expect("open_env");

    // (2) Wrap it through the env-injection seam.
    let lmdb = Lmdb::with_env(env.clone()).expect("with_env");

    // (3) Build + sign a real event.
    let keys = Keys::generate();
    let event = EventBuilder::text_note("env-injection seam works")
        .sign_with_keys(&keys)
        .expect("sign");

    // (4) Drive `save_event_with_txn` from outside the crate on a caller-
    //     opened `RwTxn`.
    {
        let mut fbb = FlatBufferBuilder::with_capacity(4096);
        let mut txn = lmdb.write_txn().expect("write_txn");
        let status = lmdb
            .save_event_with_txn(&mut txn, &mut fbb, &event)
            .expect("save");
        assert!(matches!(status, SaveEventStatus::Success));
        txn.commit().expect("commit");
    }

    // (5) Read it back via the same env — fresh `RoTxn` from the wrapper.
    {
        let rotxn = lmdb.read_txn().expect("read_txn");
        let stored = lmdb
            .get_event_by_id(&rotxn, event.id.as_bytes())
            .expect("get");
        assert!(stored.is_some(), "event missing after commit");
    }
}

#[test]
fn nmp_side_subdb_commits_atomically_with_event_write() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let env = Lmdb::open_env(tmp.path(), TEST_MAP_SIZE, TEST_MAX_READERS, TEST_ADDITIONAL_DBS)
        .expect("open_env");
    let lmdb = Lmdb::with_env(env.clone()).expect("with_env");

    // NMP opens its own sub-db on the same env. This is the load-bearing
    // claim of ADR-0011: NMP's secondary indexes share a transaction with
    // event writes.
    let nmp_sidecar = {
        let mut txn = env.write_txn().expect("env write_txn");
        let db: heed::Database<Bytes, Bytes> = env
            .database_options()
            .types::<Bytes, Bytes>()
            .name("nmp-watermarks")
            .create(&mut txn)
            .expect("create sub-db");
        txn.commit().expect("commit sub-db creation");
        db
    };

    let keys = Keys::generate();
    let event = EventBuilder::text_note("atomic commit proof")
        .sign_with_keys(&keys)
        .expect("sign");

    // Single `RwTxn` holds both the event write and the NMP-side write.
    {
        let mut fbb = FlatBufferBuilder::with_capacity(4096);
        let mut txn = lmdb.write_txn().expect("write_txn");

        // 5a — upstream event write.
        let status = lmdb
            .save_event_with_txn(&mut txn, &mut fbb, &event)
            .expect("save");
        assert!(matches!(status, SaveEventStatus::Success));

        // 5b — NMP-side write on the SAME txn.
        nmp_sidecar
            .put(&mut txn, b"relay:wss://r.test", b"\x00\x00\x00\x05")
            .expect("put watermark");

        // 5c — commit BOTH atomically.
        txn.commit().expect("atomic commit");
    }

    // Both writes survive the commit boundary.
    {
        let rotxn = lmdb.read_txn().expect("read_txn");
        assert!(lmdb
            .get_event_by_id(&rotxn, event.id.as_bytes())
            .expect("get")
            .is_some());
        let wm = nmp_sidecar.get(&rotxn, b"relay:wss://r.test").expect("get wm");
        assert_eq!(wm, Some(&b"\x00\x00\x00\x05"[..]));
    }
}

#[test]
fn additional_dbs_slot_reservation_prevents_max_dbs_exhaustion() {
    // The 11-db internal budget plus 4 additional slots leaves room for
    // 4 NMP sub-dbs on top of upstream's 11. Confirm we can actually
    // create that many without `heed::Error::DatabasesFull`.
    let tmp = tempfile::tempdir().expect("tempdir");
    let env =
        Lmdb::open_env(tmp.path(), TEST_MAP_SIZE, TEST_MAX_READERS, 4).expect("open_env");
    let _lmdb = Lmdb::with_env(env.clone()).expect("with_env"); // consumes 11 slots

    let mut txn = env.write_txn().expect("write_txn");
    for name in ["watermarks", "claims", "provenance", "domain"] {
        env.database_options()
            .types::<Bytes, Bytes>()
            .name(name)
            .create(&mut txn)
            .unwrap_or_else(|e| panic!("could not create sub-db `{name}`: {e:?}"));
    }
    txn.commit().expect("commit");
}
