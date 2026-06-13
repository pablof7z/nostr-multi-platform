//! V-52 relay-origin reverse-index lookup for the LMDB backend.
//!
//! Split out of `query.rs` to keep that file under the 500 LOC hard cap.
//! The single entry point is re-exported from the `query` module so callers
//! continue to use `query::list_events_seen_on`.

use std::ops::Bound;
use std::sync::Arc;

use super::{provenance, Inner};
use crate::types::EventId;
use crate::StoreError;

/// Return the ids of events whose provenance includes `relay_url`.
///
/// O(events-on-relay): a prefix range scan over the `nmp-relay-index` sub-db
/// for keys `relay_url || 0x00 || event_id(32)`.  The bounds
/// `[relay||0x00 .. relay||0x01)` isolate exactly one relay's entries — the
/// scan never touches events from other relays (no O(store) provenance scan).
pub(super) fn list_events_seen_on(
    inner: &Arc<Inner>,
    relay_url: &str,
) -> Result<Vec<EventId>, StoreError> {
    let txn = inner
        .env
        .read_txn()
        .map_err(|e| StoreError::Io(format!("read_txn: {e}")))?;
    let (lo, hi) = provenance::relay_index_prefix_bounds(relay_url);
    let range = (Bound::Included(lo.as_slice()), Bound::Excluded(hi.as_slice()));
    let mut out = Vec::new();
    for entry in inner
        .relay_index
        .range(&txn, &range)
        .map_err(|e| StoreError::Io(format!("relay_index range: {e}")))?
    {
        let (k, _) = entry.map_err(|e| StoreError::Io(format!("relay_index step: {e}")))?;
        if let Some(id) = provenance::relay_index_id_from_key(k, relay_url.len()) {
            out.push(id);
        }
    }
    Ok(out)
}
