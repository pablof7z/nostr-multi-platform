//! Streaming query helpers: `build_filter` + `run_filter_visit`.
//!
//! Split from `query.rs` to stay within the 500-line file-size gate.

use std::ops::ControlFlow;
use std::sync::Arc;

use nostr::prelude::*;

use super::{conv, Inner};
use crate::types::{StoreQuery, StoredEvent};
use crate::StoreError;

// ─── Materialization counter (test-support) ───────────────────────────────────
//
// Counts how many LMDB events were deserialized (`EventBorrow → StoredEvent`)
// per `run_filter_visit` call.  Exposed under `test-support` so integration
// tests in `nmp-testing` can assert that early-`Break` visits do not
// over-convert the corpus.  Never compiled into production binaries.

#[cfg(any(test, feature = "test-support"))]
pub static CONVERSION_COUNT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Reset the materialization counter to zero.
///
/// Call before each sub-test that asserts on the count.
#[cfg(any(test, feature = "test-support"))]
pub fn reset_conversion_count() {
    CONVERSION_COUNT.store(0, std::sync::atomic::Ordering::Relaxed);
}

/// Return the current materialization count since the last reset.
#[cfg(any(test, feature = "test-support"))]
pub fn conversion_count() -> usize {
    CONVERSION_COUNT.load(std::sync::atomic::Ordering::Relaxed)
}

// ─── Filter construction ─────────────────────────────────────────────────────

/// Build a `nostr::Filter` for the given query.
///
/// Returns `None` for empty-set short-circuits (empty `kinds`, empty `authors`),
/// indicating that the caller should visit nothing.
pub(crate) fn build_filter(query: &StoreQuery) -> Option<Filter> {
    match query {
        StoreQuery::AuthorKind {
            author,
            kinds,
            since,
            until,
        } => {
            if kinds.is_empty() {
                return None;
            }
            let pk = PublicKey::from_slice(author).ok()?;
            let mut f = Filter::new()
                .author(pk)
                .kinds(kinds.iter().map(|k| Kind::from(*k as u16)));
            if let Some(s) = since {
                f = f.since(Timestamp::from_secs(*s));
            }
            if let Some(u) = until {
                f = f.until(Timestamp::from_secs(*u));
            }
            Some(f)
        }
        StoreQuery::AuthorsKind {
            authors,
            kinds,
            since,
            until,
        } => {
            if authors.is_empty() || kinds.is_empty() {
                return None;
            }
            let pks: Vec<PublicKey> = authors
                .iter()
                .filter_map(|a| PublicKey::from_slice(a).ok())
                .collect();
            if pks.len() != authors.len() {
                return None;
            }
            let mut f = Filter::new()
                .authors(pks)
                .kinds(kinds.iter().map(|k| Kind::from(*k as u16)));
            if let Some(s) = since {
                f = f.since(Timestamp::from_secs(*s));
            }
            if let Some(u) = until {
                f = f.until(Timestamp::from_secs(*u));
            }
            Some(f)
        }
        StoreQuery::KindTime {
            kinds,
            since,
            until,
        } => {
            let mut f = Filter::new();
            if !kinds.is_empty() {
                f = f.kinds(kinds.iter().map(|k| Kind::from(*k as u16)));
            }
            if let Some(s) = since {
                f = f.since(Timestamp::from_secs(*s));
            }
            if let Some(u) = until {
                f = f.until(Timestamp::from_secs(*u));
            }
            Some(f)
        }
        StoreQuery::KindDtag {
            kind,
            d_tag,
            since,
            until,
        } => {
            let d_str = String::from_utf8_lossy(d_tag).into_owned();
            let mut f = Filter::new()
                .kind(Kind::from(*kind as u16))
                .identifier(d_str);
            if let Some(s) = since {
                f = f.since(Timestamp::from_secs(*s));
            }
            if let Some(u) = until {
                f = f.until(Timestamp::from_secs(*u));
            }
            Some(f)
        }
        StoreQuery::Etag { target, kinds } => {
            let target = nostr::EventId::from_slice(target).ok()?;
            let mut f = Filter::new().event(target);
            if !kinds.is_empty() {
                f = f.kinds(kinds.iter().map(|k| Kind::from(*k as u16)));
            }
            Some(f)
        }
        StoreQuery::Ptag { target, kinds } => {
            let pk = PublicKey::from_slice(target).ok()?;
            let mut f = Filter::new().pubkey(pk);
            if !kinds.is_empty() {
                f = f.kinds(kinds.iter().map(|k| Kind::from(*k as u16)));
            }
            Some(f)
        }
    }
}

// ─── Streaming visitor ───────────────────────────────────────────────────────

/// Streaming backend for `query_visit`: converts events lazily, one row at a
/// time from the cursor.  `ControlFlow::Break` stops conversions immediately —
/// unlike the old `run_filter` approach that paid ~`limit` round-trips even if
/// the visitor broke at row 1.
///
/// Ordering: the fork's BTreeSet delivers `(created_at desc, id asc)` which
/// already matches the `MemEventStore` contract, so no tie-group reordering is
/// needed.  We buffer a single tie-group only to respect the `limit` cap while
/// preserving the guarantee that a partial group is still id-asc.
pub(crate) fn run_filter_visit(
    inner: &Arc<Inner>,
    filter: Filter,
    limit: usize,
    visitor: &mut dyn FnMut(&StoredEvent) -> ControlFlow<()>,
) -> Result<(), StoreError> {
    let txn = inner
        .lmdb
        .read_txn()
        .map_err(|e| StoreError::Io(format!("read_txn: {e}")))?;
    let filter = filter.limit(limit);
    let iter = inner
        .lmdb
        .query(&txn, filter)
        .map_err(|e| StoreError::Io(format!("query: {e}")))?;

    let mut total_visited: usize = 0;

    for ev_borrow in iter {
        if total_visited >= limit {
            break;
        }
        let owned: Event = ev_borrow.into_owned();
        let raw = conv::nostr_to_raw(&owned)?;
        #[cfg(any(test, feature = "test-support"))]
        CONVERSION_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let stored = conv::stored_from_raw(raw, 0);

        total_visited += 1;
        if let ControlFlow::Break(()) = visitor(&stored) {
            break;
        }
    }

    Ok(())
}
