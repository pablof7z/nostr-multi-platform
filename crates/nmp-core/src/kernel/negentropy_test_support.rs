use super::*;

impl Kernel {
    // ─── GAP-5 negentropy session-stats test accessor ─────────────────────────
    // Exposed via `test-support` feature so `nmp-nip77`'s runtime tests can
    // assert that a real reconciliation pushed non-zero stats to the kernel.

    /// Number of NEG-MSG round-trips recorded in the most-recent negentropy
    /// session.  `0` until the first session completes.
    pub fn negentropy_sync_rounds_for_test(&self) -> u64 {
        self.negentropy_sync_stats.rounds
    }

    /// Total have-IDs from the most-recent negentropy session.
    pub fn negentropy_sync_have_ids_for_test(&self) -> u64 {
        self.negentropy_sync_stats.have_ids
    }

    /// Total need-IDs from the most-recent negentropy session.
    pub fn negentropy_sync_need_ids_for_test(&self) -> u64 {
        self.negentropy_sync_stats.need_ids
    }

    /// Estimated bytes saved (not re-fetched) in the most-recent negentropy
    /// session.  Kernel-computed as `(local − have) × AVG_EVENT_BYTES`.
    pub fn negentropy_sync_transfer_avoided_bytes_for_test(&self) -> u64 {
        self.negentropy_sync_stats.transfer_avoided_bytes
    }
}
