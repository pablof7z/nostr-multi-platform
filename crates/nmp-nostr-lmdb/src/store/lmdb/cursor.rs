// Copyright (c) 2024 Michael Dilger
// Copyright (c) 2022-2023 Yuki Kishimoto
// Copyright (c) 2023-2025 Rust Nostr Developers
// Distributed under the MIT software license

//! Cursor iteration helpers, count, and event-lookup operations for the LMDB
//! event store. These are the low-level index-range accessors used by the
//! query dispatch layer (`query.rs`).

use std::ops::Bound;

use heed::types::Bytes;
use heed::{RoRange, RoTxn};
use nostr::event::borrow::EventBorrow;
use nostr::nips::nip01::Coordinate;
use nostr::{Alphabet, Filter, Kind, PublicKey, SingleLetterTag, Timestamp};

use super::super::error::Error;
use super::index;
use super::{EVENT_ID_ALL_ZEROS, EVENT_ID_ALL_255, Lmdb};

impl Lmdb {
    #[must_use]
    pub fn count(&self, txn: &RoTxn, filter: Filter) -> Result<usize, Error> {
        // Check if we can use fast counting
        let can_fast_count: bool = filter.ids.is_none()
            && filter.authors.is_none()
            && filter.kinds.is_none()
            && filter.search.is_none()
            && filter.generic_tags.is_empty();

        if !can_fast_count {
            // Complex filter - need to iterate
            return Ok(self.query(txn, filter)?.count());
        }

        // Empty filter with maybe a limit
        let empty_with_maybe_limit = Filter {
            limit: filter.limit,
            ..Default::default()
        };

        // Empty filter with no time constraints = O(1) using index length
        if filter == empty_with_maybe_limit {
            let total: usize = usize::try_from(self.ci_index.len(txn)?).unwrap_or(usize::MAX);
            return Ok(match filter.limit {
                Some(limit) => total.min(limit), // Return min of limit and total
                None => total,
            });
        }

        // Fast counting for time-based filters only
        let since: Timestamp = filter.since.unwrap_or_else(Timestamp::min);
        let until: Timestamp = filter.until.unwrap_or_else(Timestamp::max);
        let limit: Option<usize> = filter.limit;

        // Time-based filter: iterate ci_index (already sorted by time)
        let count = match limit {
            Some(limit) => self.ci_iter(txn, since, until)?.take(limit).count(),
            None => self.ci_iter(txn, since, until)?.count(),
        };

        Ok(count)
    }

    pub fn find_replaceable_event<'a>(
        &self,
        txn: &'a RoTxn,
        author: &PublicKey,
        kind: Kind,
    ) -> Result<Option<EventBorrow<'a>>, Error> {
        if !kind.is_replaceable() {
            return Err(Error::WrongEventKind);
        }

        let mut iter = self.akc_iter(
            txn,
            author.as_bytes(),
            kind.as_u16(),
            Timestamp::min(),
            Timestamp::max(),
        )?;

        if let Some(result) = iter.next() {
            let (_key, id) = result?;
            return self.get_event_by_id(txn, id);
        }

        Ok(None)
    }

    pub fn find_addressable_event<'a>(
        &'a self,
        txn: &'a RoTxn,
        addr: &Coordinate,
    ) -> Result<Option<EventBorrow<'a>>, Error> {
        if !addr.kind.is_addressable() {
            return Err(Error::WrongEventKind);
        }

        let iter = self.atc_iter(
            txn,
            addr.public_key.as_bytes(),
            &SingleLetterTag::lowercase(Alphabet::D),
            &addr.identifier,
            Timestamp::min(),
            Timestamp::max(),
        )?;

        for result in iter {
            let (_key, id) = result?;
            let event = self.get_event_by_id(txn, id)?.ok_or(Error::NotFound)?;

            // the atc index doesn't have kind, so we have to compare the kinds
            if event.kind != addr.kind.as_u16() {
                continue;
            }

            return Ok(Some(event));
        }

        Ok(None)
    }

    pub fn ci_iter<'a>(
        &self,
        txn: &'a RoTxn,
        since: Timestamp,
        until: Timestamp,
    ) -> Result<RoRange<'a, Bytes, Bytes>, Error> {
        let start_prefix = index::make_ci_index_key(until, &EVENT_ID_ALL_ZEROS);
        let end_prefix = index::make_ci_index_key(since, &EVENT_ID_ALL_255);
        let range = (
            Bound::Included(start_prefix.as_slice()),
            Bound::Excluded(end_prefix.as_slice()),
        );
        Ok(self.ci_index.range(txn, &range)?)
    }

    pub fn tc_iter<'a>(
        &self,
        txn: &'a RoTxn,
        tag_name: &SingleLetterTag,
        tag_value: &str,
        since: Timestamp,
        until: Timestamp,
    ) -> Result<RoRange<'a, Bytes, Bytes>, Error> {
        let start_prefix = index::make_tc_index_key(
            *tag_name,
            tag_value,
            until, // scan goes backwards in time
            &EVENT_ID_ALL_ZEROS,
        );
        let end_prefix = index::make_tc_index_key(*tag_name, tag_value, since, &EVENT_ID_ALL_255);
        let range = (
            Bound::Included(start_prefix.as_slice()),
            Bound::Excluded(end_prefix.as_slice()),
        );
        Ok(self.tc_index.range(txn, &range)?)
    }

    pub fn ac_iter<'a>(
        &self,
        txn: &'a RoTxn,
        author: &[u8; 32],
        since: Timestamp,
        until: Timestamp,
    ) -> Result<RoRange<'a, Bytes, Bytes>, Error> {
        let start_prefix = index::make_ac_index_key(author, until, &EVENT_ID_ALL_ZEROS);
        let end_prefix = index::make_ac_index_key(author, since, &EVENT_ID_ALL_255);
        let range = (
            Bound::Included(start_prefix.as_slice()),
            Bound::Excluded(end_prefix.as_slice()),
        );
        Ok(self.ac_index.range(txn, &range)?)
    }

    pub fn akc_iter<'a>(
        &self,
        txn: &'a RoTxn,
        author: &[u8; 32],
        kind: u16,
        since: Timestamp,
        until: Timestamp,
    ) -> Result<RoRange<'a, Bytes, Bytes>, Error> {
        let start_prefix = index::make_akc_index_key(author, kind, until, &EVENT_ID_ALL_ZEROS);
        let end_prefix = index::make_akc_index_key(author, kind, since, &EVENT_ID_ALL_255);
        let range = (
            Bound::Included(start_prefix.as_slice()),
            Bound::Excluded(end_prefix.as_slice()),
        );
        Ok(self.akc_index.range(txn, &range)?)
    }

    pub fn kc_iter<'a>(
        &self,
        txn: &'a RoTxn,
        kind: u16,
        since: Timestamp,
        until: Timestamp,
    ) -> Result<RoRange<'a, Bytes, Bytes>, Error> {
        let start_prefix = index::make_kc_index_key(kind, until, &EVENT_ID_ALL_ZEROS);
        let end_prefix = index::make_kc_index_key(kind, since, &EVENT_ID_ALL_255);
        let range = (
            Bound::Included(start_prefix.as_slice()),
            Bound::Excluded(end_prefix.as_slice()),
        );
        Ok(self.kc_index.range(txn, &range)?)
    }

    pub fn atc_iter<'a>(
        &self,
        txn: &'a RoTxn,
        author: &[u8; 32],
        tag_name: &SingleLetterTag,
        tag_value: &str,
        since: Timestamp,
        until: Timestamp,
    ) -> Result<RoRange<'a, Bytes, Bytes>, Error> {
        let start_prefix: Vec<u8> = index::make_atc_index_key(
            author,
            *tag_name,
            tag_value,
            until, // scan goes backwards in time
            &EVENT_ID_ALL_ZEROS,
        );
        let end_prefix: Vec<u8> =
            index::make_atc_index_key(author, *tag_name, tag_value, since, &EVENT_ID_ALL_255);
        let range = (
            Bound::Included(start_prefix.as_slice()),
            Bound::Excluded(end_prefix.as_slice()),
        );
        Ok(self.atc_index.range(txn, &range)?)
    }

    pub fn ktc_iter<'a>(
        &self,
        txn: &'a RoTxn,
        kind: u16,
        tag_name: &SingleLetterTag,
        tag_value: &str,
        since: Timestamp,
        until: Timestamp,
    ) -> Result<RoRange<'a, Bytes, Bytes>, Error> {
        let start_prefix = index::make_ktc_index_key(
            kind,
            *tag_name,
            tag_value,
            until, // scan goes backwards in time
            &EVENT_ID_ALL_ZEROS,
        );
        let end_prefix =
            index::make_ktc_index_key(kind, *tag_name, tag_value, since, &EVENT_ID_ALL_255);
        let range = (
            Bound::Included(start_prefix.as_slice()),
            Bound::Excluded(end_prefix.as_slice()),
        );
        Ok(self.ktc_index.range(txn, &range)?)
    }
}
