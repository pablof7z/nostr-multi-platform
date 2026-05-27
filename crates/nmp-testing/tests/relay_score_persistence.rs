//! W2 integration test — relay-author scores survive a kernel restart.
//!
//! Scenario:
//! 1. Open kernel with an injected `LmdbRelayAuthorScoreStore`.
//! 2. Record a hit for `(pubkey, relay_url)`.
//! 3. Flush (dirty → clean).
//! 4. Drop the kernel.
//! 5. Re-open kernel with a fresh store at the same path.
//! 6. Assert that the score cell's `weight(now)` > 0 — it survived.
//!
//! Requires the `lmdb-backend` feature. Run with:
//!   cargo test -p nmp-testing --features lmdb-backend --test relay_score_persistence

#![cfg(feature = "lmdb-backend")]

use nmp_core::relay_score::ClaimOutcome;

fn lmdb_capacity_available(path: &std::path::Path) -> bool {
    match nmp_core::store::LmdbEventStore::open(path) {
        Ok(store) => {
            drop(store);
            true
        }
        Err(err) => {
            let message = format!("{err:?}");
            if message.contains("No space left on device") {
                eprintln!("skipping relay score persistence test: {message}");
                return false;
            }
            panic!("LMDB open failed before relay score persistence test: {message}");
        }
    }
}

#[test]
fn scores_survive_kernel_restart() {
    let dir = tempfile::tempdir().expect("tempdir");
    if !lmdb_capacity_available(dir.path()) {
        return;
    }
    let path = dir.path().to_str().expect("path").to_string();

    let pk_hex = "b".repeat(64);
    let relay_url = "wss://r.test.example";
    let now_s = 1_700_000_000u64;

    // ─── Session 1: record + flush ────────────────────────────────────────────
    {
        let mut k = nmp_core::Kernel::with_storage_path(100, Some(&path));
        k.record_relay_score(&pk_hex, relay_url, ClaimOutcome::Hit, now_s);

        assert!(
            k.test_relay_score_dirty(),
            "map should be dirty after record"
        );
        k.flush_relay_scores_if_dirty();
        assert!(
            !k.test_relay_score_dirty(),
            "map should be clean after flush"
        );
    }

    // ─── Session 2: reload + assert ───────────────────────────────────────────
    {
        let k = nmp_core::Kernel::with_storage_path(100, Some(&path));

        let score = k.get_relay_score(&pk_hex, relay_url);
        assert!(
            score.weight(now_s) > 0.0,
            "score weight must be > 0 after reload (got {:.4})",
            score.weight(now_s)
        );
        assert!(score.successes > 0, "successes must survive restart");
    }
}
