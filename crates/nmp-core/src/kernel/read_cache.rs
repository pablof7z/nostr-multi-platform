//! Timeline read-cache entry.
//!
//! Owns [`StoredEvent`], the lightweight per-event row the kernel keeps for
//! timeline ordering and display. The `EventStore` is the single authoritative
//! writer (D4); this struct is populated only after `EventStore::insert`
//! returns `Inserted | Replaced`.

/// Lightweight read-cache entry for timeline ordering and display.
///
/// The `EventStore` is the single authoritative writer (D4).  This struct is
/// populated **only** after `EventStore::insert` returns `Inserted | Replaced`.
#[derive(Clone, Debug)]
pub(super) struct StoredEvent {
    pub(super) id: String,
    pub(super) author: String,
    pub(super) kind: u32,
    pub(super) created_at: u64,
    pub(super) tags: Vec<Vec<String>>,
    pub(super) content: String,
    pub(super) relay_count: u32,
}
