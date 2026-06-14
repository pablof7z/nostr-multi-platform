//! Kind:0 (profile metadata) ingest.

use super::super::{parse_profile, Kernel, NostrEvent};

impl Kernel {
    /// Ingest a kind:0 profile metadata event into the local read-cache.
    ///
    /// Only called after `verify_and_persist` returns `Inserted | Replaced` (D4).
    /// Uses strict `>` on `created_at` with lexicographic event-id tiebreak,
    /// mirroring the store's supersession logic.
    pub(in crate::kernel) fn ingest_profile(&mut self, event: NostrEvent) {
        let candidate = parse_profile(&event);
        let should_replace = self.profiles.get(&event.pubkey).is_none_or(|current| {
            candidate.created_at > current.created_at
                || (candidate.created_at == current.created_at
                    && candidate.event_id < current.event_id)
        });

        if should_replace {
            self.profiles.insert(event.pubkey.clone(), candidate);
            self.cached_estimated_store_bytes.set(None);
            // ADR-0055 Rung 1: bump profiles_ver + diagnostics_inputs_ver.
            // Also bump claimed_event_content_ver when event_claims is non-empty
            // (codex #1 condition 3 — enrichment dependency).
            self.projection_rev_tracker.source_versions.bump_profiles();
            if !self.event_claims.is_empty() {
                self.projection_rev_tracker.source_versions.bump_claimed_event_content();
            }
        }
    }
}
