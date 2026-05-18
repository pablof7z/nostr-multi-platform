//! `ZapsDomain` — reverse-index of zap receipts (kind:9735) keyed by their
//! zapped event id (when present).

use nmp_core::store::{DomainHandle, StoreError, StoredEvent};
use nmp_core::substrate::{DomainIndex, DomainMigration, DomainModule, DomainRegistry};

use crate::decode::{try_from_event, ZapReceiptRecord};
use crate::kinds::KIND_ZAP_RECEIPT;

pub const NAMESPACE: &str = "nmp.nip57.zaps";

const INGEST_KINDS: &[u32] = &[KIND_ZAP_RECEIPT];

pub struct ZapsDomain;

impl DomainModule for ZapsDomain {
    const NAMESPACE: &'static str = "nmp.nip57.zaps";
    const SCHEMA_VERSION: u32 = 1;

    fn ingest_kinds() -> &'static [u32] {
        INGEST_KINDS
    }

    fn migrations() -> Vec<DomainMigration> {
        Vec::new()
    }

    fn indexes() -> Vec<DomainIndex> {
        Vec::new()
    }

    fn register(registry: &mut DomainRegistry) {
        registry.register_record::<ZapReceiptRecord>();
    }
}

pub mod keys {
    /// `z\x00<zapped_event_id>\x00<receipt_id>` → empty value.
    pub const BY_TARGET_PREFIX: &[u8] = b"z\x00";

    pub fn by_target(target_id: &str, receipt_id: &str) -> Vec<u8> {
        let mut key = BY_TARGET_PREFIX.to_vec();
        key.extend_from_slice(target_id.as_bytes());
        key.push(0u8);
        key.extend_from_slice(receipt_id.as_bytes());
        key
    }

    pub fn by_target_prefix(target_id: &str) -> Vec<u8> {
        let mut key = BY_TARGET_PREFIX.to_vec();
        key.extend_from_slice(target_id.as_bytes());
        key.push(0u8);
        key
    }
}

/// Decode + index the receipt under its `zapped_event_id`. Receipts without an
/// `e` tag (zaps to a profile, addressable target, etc.) are not indexed here
/// — they need an `nmp.nip57.zaps_by_address` / `_by_profile` sibling, kept
/// out of scope.
pub fn decode_and_route(event: &StoredEvent, handle: &DomainHandle) -> Result<(), StoreError> {
    let Some(record) = try_from_event(event) else {
        return Ok(());
    };
    let Some(target) = record.zapped_event_id.as_deref() else {
        return Ok(());
    };
    handle.put(&keys::by_target(target, &record.event_id), &[])?;
    Ok(())
}

pub fn list_by_target(handle: &DomainHandle, target_id: &str) -> Result<Vec<String>, StoreError> {
    let prefix = keys::by_target_prefix(target_id);
    let entries = handle.scan_prefix(&prefix)?;
    let mut ids = Vec::new();
    for entry in entries {
        let (key, _value) = entry?;
        let header_len = prefix.len();
        if key.len() <= header_len {
            continue;
        }
        let bytes = &key[header_len..];
        let receipt_id = std::str::from_utf8(bytes)
            .map_err(|e| StoreError::Io(format!("non-utf8 receipt id in by_target index: {e}")))?;
        ids.push(receipt_id.to_string());
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_namespace_matches_constant() {
        assert_eq!(<ZapsDomain as DomainModule>::NAMESPACE, NAMESPACE);
    }

    #[test]
    fn module_ingest_kinds_returns_9735_only() {
        assert_eq!(ZapsDomain::ingest_kinds(), &[KIND_ZAP_RECEIPT]);
    }

    #[test]
    fn key_distinct_targets_do_not_alias() {
        assert_ne!(
            keys::by_target("ali", "cece"),
            keys::by_target("alice", "ce")
        );
    }
}
