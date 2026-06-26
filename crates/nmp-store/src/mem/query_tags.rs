//! Generic single-letter tag matching for `MemEventStore`.
//!
//! Split from `mem/query.rs` to keep that file under the 500-LOC hard cap.
//! `event_matches_tag_query` is the single source of truth for
//! [`crate::types::StoreQuery::Tags`] matching — both [`scan_by_tags`] and the
//! `query_visit` predicate in `mem/query.rs` call it, so the scan path and the
//! streaming path can never drift.

use std::collections::{BTreeMap, BTreeSet};

use nostr::SingleLetterTag;

use super::{bytes_to_hex, MemEventStore};
use crate::events::EventIter;
use crate::types::{PubKey, StoredEvent};
use crate::StoreError;

/// Does `ev` satisfy a [`StoreQuery::Tags`](crate::types::StoreQuery::Tags)?
///
/// Empty `authors` = any author; empty `kinds` = any kind; `since`/`until`
/// inclusive; each `(tag, values)` requires at least one raw tag row whose first
/// element is the single letter and whose second element is one of `values`
/// (AND across keys, OR within a key's values). An empty `tags` map, or any
/// empty value set, matches nothing.
pub(super) fn event_matches_tag_query(
    ev: &StoredEvent,
    authors: &BTreeSet<PubKey>,
    kinds: &[u32],
    tags: &BTreeMap<SingleLetterTag, BTreeSet<String>>,
    since: Option<u64>,
    until: Option<u64>,
) -> bool {
    if tags.is_empty() || tags.values().any(BTreeSet::is_empty) {
        return false;
    }
    if !authors.is_empty() && !authors.iter().any(|a| bytes_to_hex(a) == ev.raw.pubkey) {
        return false;
    }
    if !kinds.is_empty() && !kinds.contains(&ev.raw.kind) {
        return false;
    }
    if since.is_some_and(|s| ev.raw.created_at < s) {
        return false;
    }
    if until.is_some_and(|u| ev.raw.created_at > u) {
        return false;
    }
    tags.iter().all(|(tag, values)| {
        let key = tag.as_str();
        ev.raw.tags.iter().any(|row| {
            row.first().is_some_and(|k| k == key)
                && row.get(1).is_some_and(|val| values.contains(val))
        })
    })
}

pub(super) fn scan_by_tags<'a>(
    store: &'a MemEventStore,
    authors: &BTreeSet<PubKey>,
    kinds: &[u32],
    tags: &BTreeMap<SingleLetterTag, BTreeSet<String>>,
    since: Option<u64>,
    until: Option<u64>,
    limit: usize,
) -> Result<Box<dyn EventIter + 'a>, StoreError> {
    let st = store.lock()?;
    let mut results: Vec<StoredEvent> = st
        .events
        .values()
        .filter(|ev| event_matches_tag_query(ev, authors, kinds, tags, since, until))
        .cloned()
        .collect();
    results.sort_by(|a, b| {
        b.raw
            .created_at
            .cmp(&a.raw.created_at)
            .then(a.raw.id.cmp(&b.raw.id))
    });
    results.truncate(limit);
    Ok(Box::new(results.into_iter().map(Ok)))
}
