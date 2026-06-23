// Copyright (c) 2024 Michael Dilger
// Copyright (c) 2022-2023 Yuki Kishimoto
// Copyright (c) 2023-2025 Rust Nostr Developers
// Distributed under the MIT software license

//! Query dispatch and pattern matching for the LMDB event store.
//! Cursor helpers, count, and find_* live in `cursor.rs`.

use std::collections::BTreeSet;
use std::iter;
use std::sync::Arc;
use std::sync::atomic::Ordering as AtomicOrdering;

use heed::RoTxn;
use nostr::event::borrow::EventBorrow;
use nostr::{Filter, Kind, Timestamp};

use super::super::error::Error;
use super::super::filter::DatabaseFilter;
use super::{Lmdb, QueryFilterPattern};

impl Lmdb {
    /// Find all events that match the filter
    pub fn query<'a>(
        &'a self,
        txn: &'a RoTxn,
        filter: Filter,
    ) -> Result<Box<dyn Iterator<Item = EventBorrow<'a>> + 'a>, Error> {
        if let (Some(since), Some(until)) = (filter.since, filter.until) {
            if since > until {
                return Ok(Box::new(iter::empty()));
            }
        }

        // We insert into a BTreeSet to keep them time-ordered
        let mut output: BTreeSet<EventBorrow<'a>> = BTreeSet::new();

        let limit: Option<usize> = filter.limit;
        let since = filter.since.unwrap_or_else(Timestamp::min);
        let until = filter.until.unwrap_or_else(Timestamp::max);

        let filter: DatabaseFilter = filter.into();

        // Identify pattern
        let pattern: QueryFilterPattern = QueryFilterPattern::from_filter(&filter);

        tracing::debug!("Querying by pattern: {pattern:?}");

        // Query by pattern
        match pattern {
            QueryFilterPattern::Ids => self.query_by_ids(txn, filter, limit, &mut output)?,
            QueryFilterPattern::AuthorsAndKinds => {
                self.query_by_authors_and_kinds(txn, filter, since, until, limit, &mut output)?;
            }
            QueryFilterPattern::AuthorsAndTags => {
                self.query_by_authors_and_tags(txn, filter, since, until, limit, &mut output)?;
            }
            QueryFilterPattern::AuthorKindsAndTags => {
                self.query_by_authors_kinds_and_tags(txn, filter, since, until, limit, &mut output)?;
            }
            QueryFilterPattern::KindsAndTags => {
                self.query_by_kinds_and_tags(txn, filter, since, until, limit, &mut output)?;
            }
            QueryFilterPattern::Tags => {
                self.query_by_tags(txn, filter, since, until, limit, &mut output)?;
            }
            QueryFilterPattern::Authors => {
                self.query_by_authors(txn, filter, since, until, limit, &mut output)?;
            }
            QueryFilterPattern::Kinds => {
                self.query_by_kinds(txn, filter, since, until, limit, &mut output)?;
            }
            QueryFilterPattern::Scraping => {
                return self.query_by_scraping(txn, filter, since, until, limit);
            }
        }

        // Optionally apply limit
        Ok(match limit {
            Some(limit) => Box::new(output.into_iter().take(limit)),
            None => Box::new(output.into_iter()),
        })
    }

    fn query_by_ids<'a>(
        &self,
        txn: &'a RoTxn,
        filter: DatabaseFilter,
        limit: Option<usize>,
        output: &mut BTreeSet<EventBorrow<'a>>,
    ) -> Result<(), Error> {
        // Fetch by id
        for id in &filter.ids {
            // Check if limit is set
            if let Some(limit) = limit {
                // Stop if limited
                if output.len() >= limit {
                    break;
                }
            }

            if let Some(event) = self.get_event_by_id(txn, id)? {
                if filter.match_event(&event) {
                    output.insert(event);
                }
            }
        }

        Ok(())
    }

    fn query_by_authors_and_kinds<'a>(
        &self,
        txn: &'a RoTxn,
        filter: DatabaseFilter,
        since: Timestamp,
        until: Timestamp,
        limit: Option<usize>,
        output: &mut BTreeSet<EventBorrow<'a>>,
    ) -> Result<(), Error> {
        // We may bring since forward if we hit the limit without going back that
        // far, so we use a mutable since:
        let mut since: Timestamp = since;

        for author in &filter.authors {
            for kind in &filter.kinds {
                let iter = self.akc_iter(txn, author, *kind, since, until)?;

                // Count how many we have found of this author-kind pair, so we
                // can possibly update `since`
                let mut paircount = 0;

                'per_event: for result in iter {
                    let (_key, value) = result?;
                    let event = self.get_event_by_id(txn, value)?.ok_or(Error::NotFound)?;

                    // If we have gone beyond since, we can stop early
                    // (We have to check because `since` might change in this loop)
                    if event.created_at < since {
                        break 'per_event;
                    }

                    // check against the rest of the filter
                    if filter.match_event(&event) {
                        let created_at = event.created_at;

                        // Accept the event
                        output.insert(event);
                        paircount += 1;

                        // Stop this pair if limited
                        if let Some(limit) = limit {
                            if paircount >= limit {
                                // Since we found the limit just among this pair,
                                // potentially move since forward
                                if created_at > since {
                                    since = created_at;
                                }
                                break 'per_event;
                            }
                        }

                        // If kind is replaceable (and not parameterized)
                        // then don't take any more events for this author-kind
                        // pair.
                        // NOTE that this optimization is difficult to implement
                        // for other replaceable event situations
                        if Kind::from(*kind).is_replaceable() {
                            break 'per_event;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn query_by_authors_and_tags<'a>(
        &self,
        txn: &'a RoTxn,
        filter: DatabaseFilter,
        since: Timestamp,
        until: Timestamp,
        limit: Option<usize>,
        output: &mut BTreeSet<EventBorrow<'a>>,
    ) -> Result<(), Error> {
        // We may bring since forward if we hit the limit without going back that
        // far, so we use a mutable since:
        let mut since: Timestamp = since;

        for author in &filter.authors {
            for (tagname, set) in &filter.generic_tags {
                for tag_value in set {
                    let iter = self
                        .atc_iter(txn, author, tagname, tag_value, since, until)?
                        .filter_map(|res| {
                            let (_k, v) = res.ok()?;
                            Some(v)
                        });
                    self.iterate_filter_until_limit(txn, &filter, iter, &mut since, limit, output)?;
                }
            }
        }

        Ok(())
    }

    fn query_by_authors_kinds_and_tags<'a>(
        &self,
        txn: &'a RoTxn,
        filter: DatabaseFilter,
        since: Timestamp,
        until: Timestamp,
        limit: Option<usize>,
        output: &mut BTreeSet<EventBorrow<'a>>,
    ) -> Result<(), Error> {
        // We may bring since forward if we hit the limit without going back that
        // far, so we use a mutable since:
        let mut since: Timestamp = since;

        for author in &filter.authors {
            for kind in &filter.kinds {
                // Author + Kind index
                let akc_iter = self.akc_iter(txn, author, *kind, since, until)?;

                // Collect Author + Kind BTree set
                let akc_set: BTreeSet<&[u8]> = akc_iter
                    .filter_map(|res| {
                        let (_k, v) = res.ok()?;
                        Some(v)
                    })
                    .collect();

                for (tagname, set) in &filter.generic_tags {
                    for tag_value in set {
                        // Author + Tag index
                        let atc_iter =
                            self.atc_iter(txn, author, tagname, tag_value, since, until)?;

                        // Collect Author + Tag BTree set
                        let atc_set: BTreeSet<&[u8]> = atc_iter
                            .filter_map(|res| {
                                let (_k, v) = res.ok()?;
                                Some(v)
                            })
                            .collect();

                        // Intersection
                        let iter = atc_set.intersection(&akc_set).copied();

                        self.iterate_filter_until_limit(
                            txn, &filter, iter, &mut since, limit, output,
                        )?;
                    }
                }
            }
        }

        Ok(())
    }

    fn query_by_kinds_and_tags<'a>(
        &self,
        txn: &'a RoTxn,
        filter: DatabaseFilter,
        since: Timestamp,
        until: Timestamp,
        limit: Option<usize>,
        output: &mut BTreeSet<EventBorrow<'a>>,
    ) -> Result<(), Error> {
        // We may bring since forward if we hit the limit without going back that
        // far, so we use a mutable since:
        let mut since: Timestamp = since;

        for kind in &filter.kinds {
            for (tag_name, set) in &filter.generic_tags {
                for tag_value in set {
                    let iter = self
                        .ktc_iter(txn, *kind, tag_name, tag_value, since, until)?
                        .filter_map(|res| {
                            let (_k, v) = res.ok()?;
                            Some(v)
                        });
                    self.iterate_filter_until_limit(txn, &filter, iter, &mut since, limit, output)?;
                }
            }
        }

        Ok(())
    }

    fn query_by_tags<'a>(
        &self,
        txn: &'a RoTxn,
        filter: DatabaseFilter,
        since: Timestamp,
        until: Timestamp,
        limit: Option<usize>,
        output: &mut BTreeSet<EventBorrow<'a>>,
    ) -> Result<(), Error> {
        // We may bring since forward if we hit the limit without going back that
        // far, so we use a mutable since:
        let mut since: Timestamp = since;

        for (tag_name, set) in &filter.generic_tags {
            for tag_value in set {
                let iter = self
                    .tc_iter(txn, tag_name, tag_value, since, until)?
                    .filter_map(|res| {
                        let (_k, v) = res.ok()?;
                        Some(v)
                    });
                self.iterate_filter_until_limit(txn, &filter, iter, &mut since, limit, output)?;
            }
        }

        Ok(())
    }

    fn query_by_authors<'a>(
        &self,
        txn: &'a RoTxn,
        filter: DatabaseFilter,
        since: Timestamp,
        until: Timestamp,
        limit: Option<usize>,
        output: &mut BTreeSet<EventBorrow<'a>>,
    ) -> Result<(), Error> {
        // We may bring since forward if we hit the limit without going back that
        // far, so we use a mutable since:
        let mut since: Timestamp = since;

        for author in &filter.authors {
            let iter = self.ac_iter(txn, author, since, until)?.filter_map(|res| {
                let (_k, v) = res.ok()?;
                Some(v)
            });
            self.iterate_filter_until_limit(txn, &filter, iter, &mut since, limit, output)?;
        }

        Ok(())
    }

    fn query_by_kinds<'a>(
        &self,
        txn: &'a RoTxn,
        filter: DatabaseFilter,
        since: Timestamp,
        until: Timestamp,
        limit: Option<usize>,
        output: &mut BTreeSet<EventBorrow<'a>>,
    ) -> Result<(), Error> {
        // We may bring since forward if we hit the limit without going back that
        // far, so we use a mutable since:
        let mut since: Timestamp = since;

        for kind in &filter.kinds {
            let iter = self.kc_iter(txn, *kind, since, until)?.filter_map(|res| {
                let (_k, v) = res.ok()?;
                Some(v)
            });
            self.iterate_filter_until_limit(txn, &filter, iter, &mut since, limit, output)?;
        }

        Ok(())
    }

    /// SCRAPE
    ///
    /// This is INEFFICIENT as it scans through many events
    pub(super) fn query_by_scraping<'a>(
        &'a self,
        txn: &'a RoTxn,
        filter: DatabaseFilter,
        since: Timestamp,
        until: Timestamp,
        limit: Option<usize>,
    ) -> Result<Box<dyn Iterator<Item = EventBorrow<'a>> + 'a>, Error> {
        // Iterate over created _at index, so events are already sorted.
        //
        // V-69 fix: the secondary `get_event_by_id` lookup can fail in two
        // distinct ways that require separate treatment:
        //
        //   Ok(None)  — the index entry points to a non-existent event row
        //               (dangling / orphan index pointer). This indicates
        //               index corruption: the index was not cleaned up when
        //               the event was removed.
        //
        //   Err(_)    — the event row exists but cannot be deserialized
        //               (FlatBuffers corruption or LMDB I/O error).
        //
        // Both cases increment a typed counter and emit a tracing::warn so
        // that silent query incompleteness becomes observable.  The iterator
        // still skips the entry (returning None from filter_map) to avoid
        // propagating an error through the iterator interface, but the
        // non-zero counter signals to callers that results may be incomplete.
        let orphan_counter = Arc::clone(&self.anomaly_orphan_index_entries);
        let unresolvable_counter = Arc::clone(&self.anomaly_unresolvable_events);
        Ok(Box::new(
            self.ci_iter(txn, since, until)?
                .filter_map(move |res| {
                    let (_key, value) = res.ok()?;
                    // Distinguish the two error classes instead of using .ok()??.
                    let event: EventBorrow = match self.get_event_by_id(txn, value) {
                        Ok(Some(e)) => e,
                        Ok(None) => {
                            // Dangling index pointer — event row is gone.
                            orphan_counter.fetch_add(1, AtomicOrdering::Relaxed);
                            tracing::warn!(
                                target: "nmp.nostr_lmdb.anomaly",
                                key = %value.iter().fold(String::new(), |mut s, b| { use std::fmt::Write; let _ = write!(s, "{b:02x}"); s }),
                                "orphan ci_index entry: event row missing (V-69 StoreAnomaly::OrphanIndexEntry)",
                            );
                            return None;
                        }
                        Err(e) => {
                            // Row exists but is unreadable / cannot be deserialized.
                            unresolvable_counter.fetch_add(1, AtomicOrdering::Relaxed);
                            tracing::warn!(
                                target: "nmp.nostr_lmdb.anomaly",
                                key = %value.iter().fold(String::new(), |mut s, b| { use std::fmt::Write; let _ = write!(s, "{b:02x}"); s }),
                                error = %e,
                                "unresolvable ci_index entry: event row undeserializable (V-69 StoreAnomaly::UnresolvableEvent)",
                            );
                            return None;
                        }
                    };

                    if filter.match_event(&event) {
                        Some(event)
                    } else {
                        None
                    }
                })
                .take(limit.unwrap_or(usize::MAX)),
        ))
    }

    pub(super) fn iterate_filter_until_limit<'a, 'i, I>(
        &self,
        txn: &'a RoTxn,
        filter: &DatabaseFilter,
        iter: I,
        since: &mut Timestamp,
        limit: Option<usize>,
        output: &mut BTreeSet<EventBorrow<'a>>,
    ) -> Result<(), Error>
    where
        I: IntoIterator<Item = &'i [u8]>,
    {
        let mut count: usize = 0;

        for id in iter {
            // Get event by ID
            let event = self.get_event_by_id(txn, id)?.ok_or(Error::NotFound)?;

            if event.created_at < *since {
                break;
            }

            // check against the rest of the filter
            if filter.match_event(&event) {
                let created_at = event.created_at;

                // Accept the event
                output.insert(event);
                count += 1;

                // Check if limit is set
                if let Some(limit) = limit {
                    // Stop if limited
                    if count >= limit {
                        if created_at > *since {
                            *since = created_at;
                        }
                        break;
                    }
                }
            }
        }

        Ok(())
    }
}
