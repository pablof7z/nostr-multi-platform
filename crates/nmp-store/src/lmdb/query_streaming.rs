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
/// This counter is process-global. Tests that assert on it must serialize the
/// full reset -> query -> assert window against sibling tests that call
/// `query_visit`.
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
        StoreQuery::Tags {
            authors,
            kinds,
            tags,
            since,
            until,
        } => build_tags_filter(authors, kinds, tags, *since, *until),
    }
}

/// Build the `nostr::Filter` for a [`StoreQuery::Tags`] scan.
///
/// Returns `None` (visit nothing) when `tags` is empty or any value set is
/// empty, or when an author hex fails to decode. `authors`/`kinds` are added
/// only when non-empty (empty = wildcard, per the `Tags` contract). Each tag
/// dimension is added via `Filter::custom_tags`, which populates the fork's
/// `generic_tags` so the `tci`/`atci`/`ktci` indexes serve the read.
pub(crate) fn build_tags_filter(
    authors: &std::collections::BTreeSet<crate::types::PubKey>,
    kinds: &[u32],
    tags: &std::collections::BTreeMap<SingleLetterTag, std::collections::BTreeSet<String>>,
    since: Option<u64>,
    until: Option<u64>,
) -> Option<Filter> {
    if tags.is_empty() || tags.values().any(std::collections::BTreeSet::is_empty) {
        return None;
    }
    let mut f = Filter::new();
    if !authors.is_empty() {
        let pks: Vec<PublicKey> = authors
            .iter()
            .filter_map(|a| PublicKey::from_slice(a).ok())
            .collect();
        if pks.len() != authors.len() {
            return None;
        }
        f = f.authors(pks);
    }
    if !kinds.is_empty() {
        f = f.kinds(kinds.iter().map(|k| Kind::from(*k as u16)));
    }
    for (tag, values) in tags {
        f = f.custom_tags(*tag, values.iter().cloned());
    }
    if let Some(s) = since {
        f = f.since(Timestamp::from_secs(s));
    }
    if let Some(u) = until {
        f = f.until(Timestamp::from_secs(u));
    }
    Some(f)
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
