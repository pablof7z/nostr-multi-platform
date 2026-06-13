use super::Serialize;

// ── Negentropy session stats ──────────────────────────────────────────────────

/// Average Nostr event size used to estimate `transfer_avoided_bytes`.
/// 512 bytes is a conservative mid-range for kind:1 / kind:3 events.
pub(super) const AVG_EVENT_BYTES: u64 = 512;

/// NIP-agnostic negentropy session statistics accumulated across one
/// reconciliation session and pushed to the kernel on completion via
/// [`crate::Kernel::set_negentropy_sync_stats`].
///
/// Kernel-owned and NIP-agnostic (D0): the concrete NIP-77 binding lives in
/// `nmp-nip77`; only raw counts cross the substrate boundary. The kernel
/// computes derived fields (`transfer_avoided_bytes`, `last_reconcile_at_ms`)
/// so neither moves to a leaf crate (D9: clock from kernel, not raw `SystemTime`).
#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct NegentropySyncStats {
    /// Number of NEG-MSG round-trips completed in the session.
    pub(super) rounds: u64,
    /// Total IDs the local client has that the relay lacks.
    pub(super) have_ids: u64,
    /// Total IDs the relay has that the local client lacks.
    pub(super) need_ids: u64,
    /// Number of local items in the reconciliation set at session open.
    pub(super) local_item_count: u64,
    /// Estimated bytes saved by not re-fetching already-local events.
    /// Computed kernel-side: `(local_item_count − have_ids) × AVG_EVENT_BYTES`.
    pub(super) transfer_avoided_bytes: u64,
    /// Kernel-clock ms at the moment the session completed (`None` until first
    /// session finishes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) last_reconcile_at_ms: Option<u64>,
}
