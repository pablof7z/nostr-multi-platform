// Copyright (c) 2024 Michael Dilger
// Copyright (c) 2022-2023 Yuki Kishimoto
// Copyright (c) 2023-2025 Rust Nostr Developers
// Distributed under the MIT software license

//! Write, mutation, and deletion operations on the LMDB event store.

use std::cmp::Ordering;

use heed::{RoTxn, RwTxn};
use nostr::event::borrow::EventBorrow;
use nostr::nips::nip01::{Coordinate, CoordinateBorrow};
use nostr::{Alphabet, Event, EventId, Kind, SingleLetterTag, Timestamp};
use nostr_database::flatbuffers::FlatBufferDecodeBorrowed;
use nostr_database::{FlatBufferBuilder, FlatBufferEncode, RejectedReason, SaveEventStatus};

use nostr::Filter;

use super::super::error::Error;
use super::index::{self, EventIndexKeys};
use super::Lmdb;

impl Lmdb {
    /// Store and index the event
    pub fn store(
        &self,
        txn: &mut RwTxn,
        fbb: &mut FlatBufferBuilder,
        event: &Event,
    ) -> Result<(), Error> {
        // Store event
        self.events
            .put(txn, event.id.as_bytes(), event.encode(fbb))?;

        // Index event
        let event: EventBorrow = EventBorrow::from(event);
        let index: EventIndexKeys = EventIndexKeys::new(event);
        self.index_event(txn, index)
    }

    pub(super) fn index_event(&self, txn: &mut RwTxn, index: EventIndexKeys) -> Result<(), Error> {
        self.ci_index.put(txn, &index.ci_index, &index.id)?;
        self.akc_index.put(txn, &index.akc_index, &index.id)?;
        self.ac_index.put(txn, &index.ac_index, &index.id)?;
        self.kc_index.put(txn, &index.kc_index, &index.id)?;

        for tag in index.tags {
            self.atc_index.put(txn, &tag.atc_index, &index.id)?;
            self.ktc_index.put(txn, &tag.ktc_index, &index.id)?;
            self.tc_index.put(txn, &tag.tc_index, &index.id)?;
        }

        Ok(())
    }

    /// Deletes an event and all its index entries using pre-collected `DeletionInfo`.
    ///
    /// This is a helper function that centralizes the deletion logic used by multiple
    /// methods (`remove_replaceable`, `remove_addressable`, `handle_deletion_event`).
    /// It eliminates code duplication and ensures all indexes are properly cleaned up.
    ///
    /// # Arguments
    /// * `txn` - The write transaction to use for deletions
    /// * `info` - Pre-collected information about the event to delete
    ///
    /// # Note
    /// This method does NOT:
    /// - Mark events as deleted (that's a semantic operation)
    /// - Verify permissions or validate the deletion
    /// - Check if the event exists
    ///
    /// It only performs the mechanical deletion from all indexes.
    pub(super) fn remove(&self, txn: &mut RwTxn, index: &EventIndexKeys) -> Result<(), Error> {
        self.events.delete(txn, &index.id)?;
        self.ci_index.delete(txn, &index.ci_index)?;
        self.akc_index.delete(txn, &index.akc_index)?;
        self.ac_index.delete(txn, &index.ac_index)?;
        self.kc_index.delete(txn, &index.kc_index)?;

        // Delete tag indexes
        for tag in &index.tags {
            self.atc_index.delete(txn, &tag.atc_index)?;
            self.ktc_index.delete(txn, &tag.ktc_index)?;
            self.tc_index.delete(txn, &tag.tc_index)?;
        }

        Ok(())
    }

    #[must_use]
    pub fn wipe(&self, txn: &mut RwTxn) -> Result<(), Error> {
        // Wipe events
        self.events.clear(txn)?;

        // Wipe indexes
        self.wipe_indexes(txn)?;

        Ok(())
    }

    fn wipe_indexes(&self, txn: &mut RwTxn) -> Result<(), Error> {
        self.ci_index.clear(txn)?;
        self.tc_index.clear(txn)?;
        self.ac_index.clear(txn)?;
        self.akc_index.clear(txn)?;
        self.kc_index.clear(txn)?;
        self.atc_index.clear(txn)?;
        self.ktc_index.clear(txn)?;
        self.deleted_ids.clear(txn)?;
        self.deleted_coordinates.clear(txn)?;
        Ok(())
    }

    #[must_use]
    #[inline]
    pub fn has_event(&self, txn: &RoTxn, event_id: &[u8; 32]) -> Result<bool, Error> {
        Ok(self.get_event_by_id(txn, event_id)?.is_some())
    }

    /// Save event with transaction support - uses single transaction for batch consistency
    pub fn save_event_with_txn(
        &self,
        txn: &mut RwTxn,
        fbb: &mut FlatBufferBuilder,
        event: &Event,
    ) -> Result<SaveEventStatus, Error> {
        if event.kind.is_ephemeral() {
            return Ok(SaveEventStatus::Rejected(RejectedReason::Ephemeral));
        }

        // Already exists
        if self.has_event(txn, event.id.as_bytes())? {
            return Ok(SaveEventStatus::Rejected(RejectedReason::Duplicate));
        }

        // Reject event if ID was deleted
        if self.is_deleted(txn, &event.id)? {
            return Ok(SaveEventStatus::Rejected(RejectedReason::Deleted));
        }

        // Reject event if ADDR was deleted after it's created_at date
        // (non-parameterized or parameterized)
        if let Some(coordinate) = event.coordinate() {
            if let Some(time) = self.when_is_coordinate_deleted(txn, &coordinate)? {
                if event.created_at <= time {
                    return Ok(SaveEventStatus::Rejected(RejectedReason::Deleted));
                }
            }
        }

        // Remove replaceable events being replaced
        if event.kind.is_replaceable() {
            if let Some(stored) = self.find_replaceable_event(txn, &event.pubkey, event.kind)? {
                if has_event_been_replaced(&stored, event) {
                    return Ok(SaveEventStatus::Rejected(RejectedReason::Replaced));
                }

                let coordinate = Coordinate::new(event.kind, event.pubkey);
                self.remove_replaceable(txn, &coordinate, event.created_at)?;
            }
        }

        // Remove addressable events being replaced
        if event.kind.is_addressable() {
            if let Some(identifier) = event.tags.identifier() {
                let coordinate = Coordinate::new(event.kind, event.pubkey).identifier(identifier);

                if let Some(stored) = self.find_addressable_event(txn, &coordinate)? {
                    if has_event_been_replaced(&stored, event) {
                        return Ok(SaveEventStatus::Rejected(RejectedReason::Replaced));
                    }

                    self.remove_addressable(txn, &coordinate, Timestamp::max())?;
                }
            }
        }

        // Handle deletion events
        if event.kind == Kind::EventDeletion {
            let invalid: bool = self.handle_deletion_event(txn, event)?;
            if invalid {
                return Ok(SaveEventStatus::Rejected(RejectedReason::InvalidDelete));
            }
        }

        self.store(txn, fbb, event)?;

        Ok(SaveEventStatus::Success)
    }

    #[inline]
    pub fn get_event_by_id<'a>(
        &self,
        txn: &'a RoTxn,
        event_id: &[u8],
    ) -> Result<Option<EventBorrow<'a>>, Error> {
        match self.events.get(txn, event_id)? {
            Some(bytes) => Ok(Some(EventBorrow::decode(bytes)?)),
            None => Ok(None),
        }
    }

    /// Delete events
    #[must_use]
    pub fn delete(&self, txn: &mut RwTxn, filter: Filter) -> Result<(), Error> {
        // First, collect all deletion info while we have immutable borrows
        let indexes: Vec<EventIndexKeys> = {
            let events = self.query(txn, filter)?;
            events
                .into_iter()
                .map(|event| EventIndexKeys::new(event))
                .collect()
        }; // All EventBorrow instances dropped here

        // Now we can safely mutate the transaction
        for index in indexes {
            self.remove(txn, &index)?;
        }

        Ok(())
    }

    #[must_use]
    #[inline]
    pub fn is_deleted(&self, txn: &RoTxn, event_id: &EventId) -> Result<bool, Error> {
        Ok(self.deleted_ids.get(txn, event_id.as_bytes())?.is_some())
    }

    #[must_use]
    pub fn mark_deleted(&self, txn: &mut RwTxn, event_id: &EventId) -> Result<(), Error> {
        self.deleted_ids.put(txn, event_id.as_bytes(), &())?;
        Ok(())
    }

    pub fn mark_coordinate_deleted(
        &self,
        txn: &mut RwTxn,
        coordinate: &CoordinateBorrow,
        when: Timestamp,
    ) -> Result<(), Error> {
        let key: Vec<u8> = index::make_coordinate_index_key(coordinate);
        self.deleted_coordinates.put(txn, &key, &when.as_secs())?;
        Ok(())
    }

    pub fn when_is_coordinate_deleted<'a>(
        &self,
        txn: &RoTxn,
        coordinate: &'a CoordinateBorrow<'a>,
    ) -> Result<Option<Timestamp>, Error> {
        let key: Vec<u8> = index::make_coordinate_index_key(coordinate);
        Ok(self
            .deleted_coordinates
            .get(txn, &key)?
            .map(Timestamp::from_secs))
    }

    /// Remove all replaceable events with the matching author-kind
    /// Kind must be a replaceable (not parameterized replaceable) event kind
    pub fn remove_replaceable(
        &self,
        txn: &mut RwTxn,
        coordinate: &Coordinate,
        until: Timestamp,
    ) -> Result<(), Error> {
        if !coordinate.kind.is_replaceable() {
            return Err(Error::WrongEventKind);
        }

        let iter = self.akc_iter(
            txn,
            coordinate.public_key.as_bytes(),
            coordinate.kind.as_u16(),
            Timestamp::zero(),
            until,
        )?;

        // Collect indexes for all events first to avoid iterator lifetime issues
        let mut indexes: Vec<EventIndexKeys> = Vec::new();

        for result in iter {
            let (_key, id) = result?;
            if let Some(event) = self.get_event_by_id(txn, id)? {
                indexes.push(EventIndexKeys::new(event));
            }
        }

        // Now perform deletions
        for index in indexes {
            self.remove(txn, &index)?;
        }

        Ok(())
    }

    /// Remove all parameterized-replaceable events with the matching author-kind-d
    /// Kind must be a parameterized-replaceable event kind
    pub fn remove_addressable(
        &self,
        txn: &mut RwTxn,
        coordinate: &Coordinate,
        until: Timestamp,
    ) -> Result<(), Error> {
        if !coordinate.kind.is_addressable() {
            return Err(Error::WrongEventKind);
        }

        let iter = self.atc_iter(
            txn,
            coordinate.public_key.as_bytes(),
            &SingleLetterTag::lowercase(Alphabet::D),
            &coordinate.identifier,
            Timestamp::min(),
            until,
        )?;

        // Collect DeletionInfo for all events first to avoid iterator lifetime issues
        let mut indexes = Vec::new();

        for result in iter {
            let (_key, id) = result?;
            if let Some(event) = self.get_event_by_id(txn, id)? {
                // Our index doesn't have Kind embedded, so we have to check it
                if event.kind == coordinate.kind.as_u16() {
                    indexes.push(EventIndexKeys::new(event));
                }
            }
        }

        // Now perform deletions
        for index in indexes {
            self.remove(txn, &index)?;
        }

        Ok(())
    }

    pub(super) fn handle_deletion_event(
        &self,
        txn: &mut RwTxn,
        event: &Event,
    ) -> Result<bool, Error> {
        // Collect DeletionInfo and EventIds for all valid targets first
        let mut deletions_to_process = Vec::new();

        for id in event.tags.event_ids() {
            if let Some(target) = self.get_event_by_id(txn, id.as_bytes())? {
                // Author must match
                if target.pubkey != event.pubkey.as_bytes() {
                    return Ok(true);
                }

                deletions_to_process.push((*id, EventIndexKeys::new(target)));
            }
        }

        // Now process all deletions
        for (id, info) in deletions_to_process {
            // Mark the event ID as deleted (for NIP-09 deletion events)
            self.mark_deleted(txn, &id)?;

            // Remove from all indexes
            self.remove(txn, &info)?;
        }

        for coordinate in event.tags.coordinates() {
            // Author must match
            if coordinate.public_key != event.pubkey {
                return Ok(true);
            }

            // Mark deleted
            self.mark_coordinate_deleted(txn, &coordinate.borrow(), event.created_at)?;

            // Remove events (up to the created_at of the deletion event)
            if coordinate.kind.is_replaceable() {
                self.remove_replaceable(txn, coordinate, event.created_at)?;
            } else if coordinate.kind.is_addressable() {
                self.remove_addressable(txn, coordinate, event.created_at)?;
            }
        }

        Ok(false)
    }
}

/// Check if the new event should replace the stored one.
pub(super) fn has_event_been_replaced(stored: &EventBorrow, event: &Event) -> bool {
    match stored.created_at.cmp(&event.created_at) {
        Ordering::Greater => true,
        Ordering::Equal => {
            // NIP-01: When timestamps are identical, keep the event with the lowest ID
            stored.id < event.id.as_bytes()
        }
        // Stored event is older than the new event, so it is not replaced yet.
        Ordering::Less => false,
    }
}
