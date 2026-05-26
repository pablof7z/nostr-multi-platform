//! EventStore-backed publish persistence.
//!
//! `PublishEngine` owns delivery state, but the bytes must survive process
//! death when the app is backed by LMDB. This adapter stores `PublishRecord`
//! rows in an `EventStore` domain namespace so the existing memory and LMDB
//! backends share the same contract.

use std::sync::Arc;

use crate::store::{DomainHandle, EventStore, StoreError};

use super::action::PublishHandle;
use super::traits::{PublishRecord, PublishStore, PublishStoreError};

const NAMESPACE: &str = "nmp.publish.records";

pub struct DomainPublishStore {
    domain: DomainHandle,
}

impl DomainPublishStore {
    #[must_use]
    pub fn open(store: Arc<dyn EventStore>) -> Result<Self, PublishStoreError> {
        let domain = store.domain_open(NAMESPACE).map_err(map_store_error)?;
        Ok(Self { domain })
    }

    fn key(handle: &PublishHandle) -> &[u8] {
        handle.as_bytes()
    }
}

impl PublishStore for DomainPublishStore {
    fn upsert(&self, record: &PublishRecord) -> Result<(), PublishStoreError> {
        let bytes = serde_json::to_vec(record)
            .map_err(|err| PublishStoreError::Backend(format!("encode publish record: {err}")))?;
        self.domain
            .put(Self::key(&record.handle), &bytes)
            .map_err(map_store_error)
    }

    fn delete(&self, handle: &PublishHandle) -> Result<(), PublishStoreError> {
        self.domain
            .delete(Self::key(handle))
            .map(|_| ())
            .map_err(map_store_error)
    }

    fn load_pending(&self) -> Result<Vec<PublishRecord>, PublishStoreError> {
        let mut records = Vec::new();
        for row in self.domain.scan_prefix(b"").map_err(map_store_error)? {
            let (_key, bytes) = row.map_err(map_store_error)?;
            let record: PublishRecord = serde_json::from_slice(&bytes).map_err(|err| {
                PublishStoreError::Backend(format!("decode publish record: {err}"))
            })?;
            if record
                .per_relay
                .iter()
                .any(|(_, state)| !state.is_terminal())
            {
                records.push(record);
            }
        }
        records.sort_by(|a, b| a.handle.cmp(&b.handle));
        Ok(records)
    }
}

fn map_store_error(err: StoreError) -> PublishStoreError {
    PublishStoreError::Backend(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publish::PerRelayState;
    use crate::store::MemEventStore;
    use crate::substrate::{SignedEvent, UnsignedEvent};

    fn record(handle: &str, state: PerRelayState) -> PublishRecord {
        PublishRecord {
            handle: handle.to_string(),
            event: SignedEvent {
                id: format!("{handle:0<64}"),
                sig: "a".repeat(128),
                unsigned: UnsignedEvent {
                    pubkey: "b".repeat(64),
                    kind: 1,
                    tags: Vec::new(),
                    content: "offline publish".to_string(),
                    created_at: 1_700_000_000,
                },
            },
            per_relay: vec![("wss://relay.test".to_string(), state)],
            pending_retries: Vec::new(),
            relay_reasons: Vec::new(),
        }
    }

    #[test]
    fn domain_publish_store_round_trips_pending_records() {
        let event_store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
        let store = DomainPublishStore::open(event_store).expect("open domain store");
        store
            .upsert(&record("pending", PerRelayState::Pending))
            .expect("write pending record");
        store
            .upsert(&record(
                "done",
                PerRelayState::Ok {
                    acked_at_ms: 1_700_000_000_000,
                },
            ))
            .expect("write terminal record");

        let pending = store.load_pending().expect("load pending records");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].handle, "pending");

        store
            .delete(&"pending".to_string())
            .expect("delete pending");
        assert!(store
            .load_pending()
            .expect("pending after delete")
            .is_empty());
    }
}
