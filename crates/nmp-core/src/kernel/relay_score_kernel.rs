//! Relay-author score store/record/lookup methods (W2/W3/W4).
//!
//! Extracted from `kernel/mod.rs` (`impl Kernel`) to honour the 500-LOC ceiling.

use super::*;

impl Kernel {
    /// W2 — inject and hydrate the relay-author-score persistence store.
    pub fn set_relay_score_store(
        &mut self,
        store: Box<dyn crate::substrate::RelayAuthorScoreStore>,
    ) {
        self.relay_score_map = relay_score::RelayAuthorScoreMap::new();
        // Hydrate the in-memory map from persistent state.
        match store.load_all() {
            Ok(cells) => {
                // Convert raw `([u8;32], String, u32, u32, u64)` tuples back
                // into substrate types.
                //
                // §8.10 / canonicalization-on-load: we canonicalize the URL
                // here even though `flush_relay_scores_if_dirty` already
                // canonicalized it before writing. This guards against old
                // rows written before a canonicalization rule change and is
                // more robust than relying on sub-db name bumps alone.
                // Duplicate `(pubkey, canonical_url)` pairs that arise from
                // a rule change are naturally deduplicated by
                // `BTreeMap::insert` in `bulk_load` (last-writer wins).
                let substrate_cells = cells.into_iter().filter_map(
                    |(pk_bytes, url, successes, failures, last_used_unix_s)| {
                        // Encode raw pubkey bytes → lowercase hex string.
                        let pk_hex: String = pk_bytes.iter().map(|b| format!("{b:02x}")).collect();
                        // crate::planner::Pubkey = String — just use the hex string directly.
                        let pk: crate::planner::Pubkey = pk_hex;
                        // Canonicalize the stored URL so that any trailing-slash
                        // split between old and new rows collapses to one cell.
                        let canonical_url =
                            crate::relay::CanonicalRelayUrl::parse_or_raw(&url).into_string();
                        Some((
                            pk,
                            canonical_url,
                            relay_score::RelayAuthorScore {
                                successes,
                                failures,
                                last_used_unix_s,
                            },
                        ))
                    },
                );
                self.relay_score_map.bulk_load(substrate_cells);
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "relay-score store: load_all failed — starting with empty map"
                );
            }
        }
        self.relay_score_store = Some(store);
    }

    /// W3 — record a relay-author score outcome; marks the map dirty for the next idle flush.
    pub fn record_relay_score(
        &mut self,
        author: &str,
        relay_url: &str,
        outcome: relay_score::ClaimOutcome,
        now_unix_s: u64,
    ) {
        self.relay_score_map
            .record(&author.to_string(), relay_url, outcome, now_unix_s);
    }

    /// W4/W5 — look up the current `RelayAuthorScore` for `(author, relay_url)`.
    #[must_use]
    pub fn get_relay_score(&self, author: &str, relay_url: &str) -> relay_score::RelayAuthorScore {
        self.relay_score_map.get(&author.to_string(), relay_url)
    }

    /// Test-only: whether the score map has unsaved mutations.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn test_relay_score_dirty(&self) -> bool {
        self.relay_score_map.is_dirty()
    }
}
