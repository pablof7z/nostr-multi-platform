//! Kind:0 (profile metadata) ingest.

use super::super::{Kernel, NostrEvent, parse_profile};

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
            self.profiles.insert(event.pubkey, candidate);
        }
    }
}
