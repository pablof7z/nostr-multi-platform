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

/// Staleness policy for `coverage()`: a watermark row is treated as
/// `CompleteAsOf` only while its `updated_at` is within this window of "now";
/// once `now - updated_at` exceeds it the row degrades to `PartialUpTo` and
/// the planner re-fetches.
///
/// 300s is a deliberate freshness/chattiness trade-off: short enough that a
/// view re-opened minutes later re-syncs, long enough that rapid
/// open/close/open cycling within a single session reuses the cached coverage
/// instead of re-issuing a REQ each time. This is *coverage policy*, not a
/// store-engine implementation detail, so it is defined once here next to the
/// `Coverage` type both store backends (`mem`, `lmdb`) project into — keeping
/// the two backends from drifting to different windows.
///
/// D9 caveat: the `coverage()` callers currently read wall-clock via a bare
/// `SystemTime::now()` because the `EventStore` trait does not yet thread the
/// kernel-owned clock into the store layer. The window value is policy and is
/// owned here; the *time source* is a known transitional site pending the
/// store-clock plumbing tracked for a later milestone.
pub const COVERAGE_STALENESS_WINDOW_SECS: u64 = 300;
