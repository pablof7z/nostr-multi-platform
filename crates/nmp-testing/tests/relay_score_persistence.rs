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
use nmp_core::store::LmdbEventStore;
use nmp_core::substrate::{RelayAuthorScoreStore, ScoreCell};
// relay_score_map is pub(crate); integration tests drive the kernel through
// the narrow public accessors (record_relay_score, get_relay_score,
// test_relay_score_dirty) which were added to satisfy D4 (D4: downstream
// code must not mutate scores or read the dirty flag directly).

/// Minimal LMDB-backed `RelayAuthorScoreStore` for the integration test
/// (and eventually for production actor wiring).
struct LmdbRelayAuthorScoreStore {
    path: String,
}

impl LmdbRelayAuthorScoreStore {
    fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
        }
    }

    fn open_store(&self) -> LmdbEventStore {
        LmdbEventStore::open(std::path::Path::new(&self.path)).expect("open lmdb store")
    }
}

impl RelayAuthorScoreStore for LmdbRelayAuthorScoreStore {
    fn load_all(&self) -> Result<Vec<ScoreCell>, Box<dyn std::error::Error>> {
        let store = self.open_store();
        let rows = nmp_core::store::relay_scores::load_all_raw(&store)?;
        Ok(rows)
    }

    fn put_batch(&mut self, cells: Vec<ScoreCell>) -> Result<(), Box<dyn std::error::Error>> {
        let store = self.open_store();
        nmp_core::store::relay_scores::put_batch_raw(&store, cells)?;
        Ok(())
    }
}

#[test]
fn scores_survive_kernel_restart() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().to_str().expect("path").to_string();

    let pk_hex = "b".repeat(64);
    let relay_url = "wss://r.test.example";
    let now_s = 1_700_000_000u64;

    // ─── Session 1: record + flush ────────────────────────────────────────────
    {
        let mut k = nmp_core::Kernel::with_storage_path(100, Some(&path));
        k.set_relay_score_store(Box::new(LmdbRelayAuthorScoreStore::new(&path)));

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
        let mut k = nmp_core::Kernel::with_storage_path(100, Some(&path));
        k.set_relay_score_store(Box::new(LmdbRelayAuthorScoreStore::new(&path)));

        let score = k.get_relay_score(&pk_hex, relay_url);
        assert!(
            score.weight(now_s) > 0.0,
            "score weight must be > 0 after reload (got {:.4})",
            score.weight(now_s)
        );
        assert!(score.successes > 0, "successes must survive restart");
    }
}
