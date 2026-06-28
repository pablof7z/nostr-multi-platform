//! V-52 relay-origin reverse-index lookup for the LMDB backend.
//!
//! Split out of `query.rs` to keep that file under the 500 LOC hard cap.
//! The single entry point is re-exported from the `query` module so callers
//! continue to use `query::list_events_seen_on`.

use std::collections::BTreeSet;
use std::ops::Bound;
use std::sync::Arc;

use super::{provenance, Inner};
use crate::types::{is_relay_provenance_private, EventId};
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
    let range = (
        Bound::Included(lo.as_slice()),
        Bound::Excluded(hi.as_slice()),
    );
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

/// #1518 — the distinct kinds a relay has served, ascending.
///
/// O(events-on-relay): a prefix range scan over `nmp-relay-kind` for keys
/// `relay_url || 0x00 || kind(BE4) || event_id(32)`.  A `BTreeSet` dedups and
/// sorts the kinds (each event contributes one key per relay, so the same kind
/// recurs once per event on the relay).
pub(super) fn relay_kind_coverage(
    inner: &Arc<Inner>,
    relay_url: &str,
) -> Result<Vec<u32>, StoreError> {
    let txn = inner
        .env
        .read_txn()
        .map_err(|e| StoreError::Io(format!("read_txn: {e}")))?;
    let lo = provenance::relay_kind_relay_lo(relay_url);
    let hi = provenance::relay_kind_relay_hi(relay_url);
    let range = (
        Bound::Included(lo.as_slice()),
        Bound::Excluded(hi.as_slice()),
    );
    let mut kinds: BTreeSet<u32> = BTreeSet::new();
    for entry in inner
        .relay_kind
        .range(&txn, &range)
        .map_err(|e| StoreError::Io(format!("relay_kind range: {e}")))?
    {
        let (k, _) = entry.map_err(|e| StoreError::Io(format!("relay_kind step: {e}")))?;
        if let Some(kind) = provenance::relay_kind_kind_from_key(k, relay_url.len()) {
            if !is_relay_provenance_private(kind) {
                kinds.insert(kind);
            }
        }
    }
    Ok(kinds.into_iter().collect())
}

/// #1518 — how many distinct events of `kind` a relay has served.
///
/// O(events-of-kind-on-relay): a prefix range scan over `nmp-relay-kind` for
/// the `relay_url || 0x00 || kind(BE4)` sub-range.  Each key is one
/// `(relay, kind, event_id)` triple, so the count of keys is the count of
/// distinct events of that kind on the relay.
pub(super) fn relay_kind_count(
    inner: &Arc<Inner>,
    relay_url: &str,
    kind: u32,
) -> Result<u64, StoreError> {
    if is_relay_provenance_private(kind) {
        return Ok(0);
    }
    let txn = inner
        .env
        .read_txn()
        .map_err(|e| StoreError::Io(format!("read_txn: {e}")))?;
    let lo = provenance::relay_kind_kind_lo(relay_url, kind);
    let hi = provenance::relay_kind_kind_hi(relay_url, kind);
    let range = (
        Bound::Included(lo.as_slice()),
        Bound::Excluded(hi.as_slice()),
    );
    let mut count: u64 = 0;
    for entry in inner
        .relay_kind
        .range(&txn, &range)
        .map_err(|e| StoreError::Io(format!("relay_kind count range: {e}")))?
    {
        entry.map_err(|e| StoreError::Io(format!("relay_kind count step: {e}")))?;
        count += 1;
    }
    Ok(count)
}
