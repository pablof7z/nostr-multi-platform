//! Default `EventStore::query_visit` dispatch — extracted from `events.rs` to
//! keep the trait definition under the file-size hard cap (#1811 FTS additions
//! pushed the combined trait over 500 LOC). The dispatch logic is unchanged;
//! it routes a [`StoreQuery`] through the matching `scan_by_*` index so the
//! index logic is never duplicated. Backends may still override `query_visit`
//! to avoid the scan's intermediate buffer (see `MemEventStore`).
//!
//! Design: `docs/design/nostrdb-notedeck-lessons.md` §2.3 (`ndb_query_visit`).

use std::ops::ControlFlow;

use crate::events::{EventIter, EventStore};
use crate::types::{StoreError, StoreQuery, StoredEvent};

/// Default implementation of [`EventStore::query_visit`]. Generic over the
/// concrete store (`?Sized` so `&dyn EventStore` works) so the single dispatch
/// body is shared by every backend that does not override the method.
pub(crate) fn query_visit_default<S: EventStore + ?Sized>(
    store: &S,
    query: &StoreQuery,
    limit: usize,
    visitor: &mut dyn FnMut(&StoredEvent) -> ControlFlow<()>,
) -> Result<(), StoreError> {
    let iter: Box<dyn EventIter + '_> = match query {
        StoreQuery::AuthorKind {
            author,
            kinds,
            since,
            until,
        } => store.scan_by_author_kind(author, kinds, *since, *until, limit)?,
        StoreQuery::AuthorsKind {
            authors,
            kinds,
            since,
            until,
        } => store.scan_by_authors_kind(authors, kinds, *since, *until, limit)?,
        StoreQuery::KindTime {
            kinds,
            since,
            until,
        } => store.scan_by_kind_time(kinds, *since, *until, limit)?,
        StoreQuery::KindDtag {
            kind,
            d_tag,
            since,
            until,
        } => store.scan_by_kind_dtag(*kind, d_tag, *since, *until, limit)?,
        StoreQuery::Tags {
            authors,
            kinds,
            tags,
            since,
            until,
        } => store.scan_by_tags(authors, kinds, tags, *since, *until, limit)?,
    };
    for item in iter {
        let ev = item?;
        if let ControlFlow::Break(()) = (visitor)(&ev) {
            // doctrine-allow: D15 — `visitor` is an `impl Fn` parameter (compile-time monomorphic), not a stored `Box<dyn Fn>` host closure; no FFI surface involved
            break;
        }
    }
    Ok(())
}
