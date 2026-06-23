// Copyright (c) 2024 Michael Dilger
// Copyright (c) 2022-2023 Yuki Kishimoto
// Copyright (c) 2023-2025 Rust Nostr Developers
// Distributed under the MIT software license

//! Database schema migration logic for the LMDB event store.

use std::cmp::Ordering;

use heed::RwTxn;
use nostr::event::borrow::EventBorrow;
use nostr_database::flatbuffers::FlatBufferDecodeBorrowed;

use super::index;
use super::super::error::{Error, MigrationError};
use super::{DB_VERSION, DB_VERSION_KEY, Lmdb};

impl Lmdb {
    /// Check database version and run migrations if needed
    pub(super) fn migrate(&self) -> Result<(), Error> {
        let mut txn = self.write_txn()?;

        // Get current database version (defaults to 0 if not set)
        let current_version: u64 = self.metadata.get(&txn, DB_VERSION_KEY)?.unwrap_or(0);

        match current_version.cmp(&DB_VERSION) {
            Ordering::Less => {
                tracing::info!(
                    "Migrating database from version {} to {}",
                    current_version,
                    DB_VERSION
                );

                // Run migrations sequentially
                if current_version < 2 {
                    self.migrate_v1_to_v2(&mut txn)?;
                }

                // Update version
                self.metadata.put(&mut txn, DB_VERSION_KEY, &DB_VERSION)?;
                txn.commit()?;

                tracing::info!("Migration completed successfully");

                Ok(())
            }
            Ordering::Equal => {
                txn.abort();
                Ok(())
            }
            Ordering::Greater => {
                txn.abort();
                Err(Error::Migration(MigrationError::NewerVersion {
                    current_version,
                    new_version: DB_VERSION,
                }))
            }
        }
    }

    /// Migrate from version 1 to version 2: Build `kc_index`
    fn migrate_v1_to_v2(&self, txn: &mut RwTxn) -> Result<(), Error> {
        tracing::info!("Building kc_index for existing events...");

        let event_count = self.events.len(txn)?;
        tracing::info!("Processing {} events", event_count);

        // Collect all kc_index keys first to avoid borrow conflicts
        let kc_indexes: Vec<(Vec<u8>, [u8; 32])> = {
            let mut indexes = Vec::with_capacity(usize::try_from(event_count).unwrap_or(usize::MAX));
            for result in self.events.iter(txn)? {
                let (_id, event_bytes) = result?;

                // Decode event
                if let Ok(event) = EventBorrow::decode(event_bytes) {
                    // Build just the kc_index key
                    let kc_index_key =
                        index::make_kc_index_key(event.kind, event.created_at, event.id);
                    indexes.push((kc_index_key, *event.id));
                }
            }
            indexes
        };

        // Now insert all the indexes
        for (kc_index_key, event_id) in kc_indexes {
            self.kc_index.put(txn, &kc_index_key, &event_id)?;
        }

        tracing::info!("kc_index built successfully");
        Ok(())
    }
}
