//! Watermark types (sync-coverage bookmarks per filter×relay pair).
//!
//! D3 (sync): watermarks are persisted across launches; Coverage drives relay
//! fetch decisions.

use super::ids::RelayUrl;

// ─── Watermarks ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub struct WatermarkKey {
    pub filter_hash: [u8; 32],
    pub relay_url: RelayUrl,
}

#[derive(Clone, Debug)]
pub struct WatermarkRow {
    pub key: WatermarkKey,
    pub synced_up_to: u64,    // unix seconds
    pub last_sync_method: SyncMethod,
    /// Engine-opaque resume blob (M4).
    pub last_negentropy_state: Option<Vec<u8>>,
    pub bytes_saved_vs_req: u64,
    pub updated_at: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncMethod {
    Negentropy,
    ReqScan,
    Manual,
}

/// Returned by `coverage()` to classify watermark freshness.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Coverage {
    /// Fully synced; a cache miss is authoritative "doesn't exist".
    CompleteAsOf(u64),
    /// Synced up to timestamp but row is stale — fetch is needed.
    PartialUpTo(u64),
    /// No watermark; always fetch.
    Unknown,
}
