//! `Kernel::flush_relay_scores_if_dirty`
//!
//! Drains the in-memory `RelayAuthorScoreMap` into the injected
//! `RelayAuthorScoreStore` when the map is dirty.
//!
//! # Doctrine
//! - D4: `&mut self` — the kernel is the sole writer.
//! - D6: LMDB errors → `tracing::warn!` + bail, never panic, never `Result`
//!   across the FFI boundary.
//! - D8: the flush is gated by `relay_score_map.dirty`; a clean map is a
//!   no-op O(1) check.
//! - Keys use `[32 pubkey bytes][1 byte URL-len u8][N URL bytes]`;
//!   URLs >255 bytes are skipped with `tracing::warn!`.
//! - URLs are canonicalized via `CanonicalRelayUrl::parse_or_raw`
//!   *before* reaching the store layer.

impl super::Kernel {
    /// Flush dirty score cells to the injected `RelayAuthorScoreStore`.
    ///
    /// Called on actor idle. No-op when the map is clean or when no
    /// store has been injected. On store error: logs + returns without marking
    /// clean so the next idle cycle retries.
    pub fn flush_relay_scores_if_dirty(&mut self) {
        let Some(store) = self.relay_score_store.as_mut() else {
            return;
        };
        if !self.relay_score_map.is_dirty() {
            return;
        }
        let cells: Vec<([u8; 32], String, u32, u32, u64)> = self
            .relay_score_map
            .snapshot()
            .into_iter()
            .filter_map(|(pk, url, score)| {
                // Canonicalize before keying.
                let canon_url = crate::relay::CanonicalRelayUrl::parse_or_raw(&url).into_string();
                // Reject URLs > 255 bytes.
                if canon_url.len() > 255 {
                    tracing::warn!(
                        relay_url = %canon_url,
                        "relay-score flush: URL exceeds 255-byte limit — skipping cell"
                    );
                    return None;
                }
                // Convert substrate Pubkey (hex string) → raw [u8;32].
                // `hex_to_pubkey_bytes` is the existing kernel helper (D6: returns None on bad hex).
                let pk_bytes = super::hex_to_pubkey_bytes(&pk)?;
                Some((
                    pk_bytes,
                    canon_url,
                    score.successes,
                    score.failures,
                    score.last_used_unix_s,
                ))
            })
            .collect();

        if let Err(e) = store.put_batch(cells) {
            tracing::warn!(error = %e, "relay-score flush: put_batch failed — will retry on next idle");
            return;
        }
        self.relay_score_map.mark_clean();
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use crate::kernel::relay_score::ClaimOutcome;
    use crate::substrate::RelayAuthorScoreStore;

    /// Minimal capturing store for flush tests. Records every `put_batch` call
    /// so tests can assert batch contents without a real LMDB env.
    struct CapturingStore {
        calls: Arc<Mutex<Vec<Vec<([u8; 32], String, u32, u32, u64)>>>>,
    }

    impl CapturingStore {
        fn new() -> Self {
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    impl RelayAuthorScoreStore for CapturingStore {
        fn load_all(
            &self,
        ) -> Result<Vec<([u8; 32], String, u32, u32, u64)>, Box<dyn std::error::Error>> {
            Ok(Vec::new())
        }

        fn put_batch(
            &mut self,
            cells: Vec<([u8; 32], String, u32, u32, u64)>,
        ) -> Result<(), Box<dyn std::error::Error>> {
            self.calls.lock().unwrap().push(cells);
            Ok(())
        }
    }

    fn make_kernel_with_capturing() -> (
        super::super::Kernel,
        Arc<Mutex<Vec<Vec<([u8; 32], String, u32, u32, u64)>>>>,
    ) {
        let mut k = super::super::Kernel::new(100);
        let store = CapturingStore::new();
        let calls = Arc::clone(&store.calls);
        k.set_relay_score_store(Box::new(store));
        (k, calls)
    }

    /// Flush is a no-op when the map has never been written to.
    ///
    /// With a fresh `RelayAuthorScoreMap` (dirty = false) the flush must not
    /// call `put_batch` at all.
    #[test]
    fn flush_is_noop_when_clean() {
        let (mut k, calls) = make_kernel_with_capturing();
        // Map is clean by default — flush must be a no-op.
        k.flush_relay_scores_if_dirty();
        assert_eq!(
            calls.lock().unwrap().len(),
            0,
            "put_batch must not be called when map is clean"
        );
    }

    /// Canonicalization consolidates trailing-slash and no-slash
    /// variant to one cell.
    ///
    /// `wss://r.example.com/` and `wss://r.example.com` both canonicalize to
    /// `wss://r.example.com` (strip-trailing-slash rule). Writing via
    /// both spellings results in one score cell.
    #[test]
    fn canonicalization_consolidates_trailing_slash_to_one_cell() {
        let (mut k, calls) = make_kernel_with_capturing();

        // Record a hit using the trailing-slash form.
        let pk_hex = "a".repeat(64);
        let now = 1_700_000_000u64;
        k.relay_score_map.record(
            &pk_hex.parse::<crate::planner::Pubkey>().expect("parse pk"),
            "wss://r.example.com/",
            ClaimOutcome::Hit,
            now,
        );

        // Flush — must produce exactly one cell for the canonical URL.
        k.flush_relay_scores_if_dirty();

        let calls_guard = calls.lock().unwrap();
        assert_eq!(calls_guard.len(), 1, "one put_batch call expected");
        let batch = &calls_guard[0];
        assert_eq!(batch.len(), 1, "one cell in the batch");
        // The canonical URL must have no trailing slash.
        assert_eq!(batch[0].1, "wss://r.example.com", "canonical URL mismatch");
    }
}
