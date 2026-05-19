//! `CommentsDomain` — `DomainModule` registration for kind:1111 (standalone).
//!
//! Mirrors the reverse-index shape used by `nmp-nip01::RepliesDomain` and
//! `nmp-nip23::ArticlesDomain`. The key is `(parent_event_id, comment_id)`
//! so a single prefix scan enumerates direct comments to a parent without
//! re-touching the event store.

use nmp_core::store::{DomainHandle, StoreError, StoredEvent};
use nmp_core::substrate::{DomainIndex, DomainMigration, DomainModule};

use crate::decode::{try_from_event, CommentPointer};
use crate::kinds::KIND_COMMENT;

pub const NAMESPACE: &str = "nmp.nip22.comments";

const INGEST_KINDS: &[u32] = &[KIND_COMMENT];

pub struct CommentsDomain;

impl DomainModule for CommentsDomain {
    const NAMESPACE: &'static str = "nmp.nip22.comments";
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
}

pub mod keys {
    /// `c\x00<parent_id>\x00<comment_id>` → empty value.
    pub const BY_PARENT_PREFIX: &[u8] = b"c\x00";

    pub fn by_parent(parent_id: &str, comment_id: &str) -> Vec<u8> {
        let mut key = BY_PARENT_PREFIX.to_vec();
        key.extend_from_slice(parent_id.as_bytes());
        key.push(0u8);
        key.extend_from_slice(comment_id.as_bytes());
        key
    }

    pub fn by_parent_prefix(parent_id: &str) -> Vec<u8> {
        let mut key = BY_PARENT_PREFIX.to_vec();
        key.extend_from_slice(parent_id.as_bytes());
        key.push(0u8);
        key
    }
}

/// Decode + index the comment under its lowercase-`e` parent. Skips events
/// whose parent isn't an `Event` pointer (Address / External targets live in
/// a separate-shape index, intentionally out of scope here).
pub fn decode_and_route(event: &StoredEvent, handle: &DomainHandle) -> Result<(), StoreError> {
    let Some(record) = try_from_event(event) else {
        return Ok(());
    };
    let parent_id = match &record.parent {
        CommentPointer::Event { id, .. } => id.clone(),
        _ => return Ok(()),
    };

    handle.put(&keys::by_parent(&parent_id, &record.event_id), &[])?;
    Ok(())
}

pub fn list_by_parent(handle: &DomainHandle, parent_id: &str) -> Result<Vec<String>, StoreError> {
    let prefix = keys::by_parent_prefix(parent_id);
    let entries = handle.scan_prefix(&prefix)?;
    let mut ids = Vec::new();
    for entry in entries {
        let (key, _value) = entry?;
        let header_len = prefix.len();
        if key.len() <= header_len {
            continue;
        }
        let comment_id_bytes = &key[header_len..];
        let comment_id = std::str::from_utf8(comment_id_bytes)
            .map_err(|e| StoreError::Io(format!("non-utf8 comment id in by_parent index: {e}")))?;
        ids.push(comment_id.to_string());
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_namespace_matches_constant() {
        assert_eq!(<CommentsDomain as DomainModule>::NAMESPACE, NAMESPACE);
    }

    #[test]
    fn module_ingest_kinds_returns_1111_only() {
        assert_eq!(CommentsDomain::ingest_kinds(), &[KIND_COMMENT]);
    }

    #[test]
    fn key_distinct_parents_do_not_alias() {
        assert_ne!(
            keys::by_parent("ali", "cece"),
            keys::by_parent("alice", "ce")
        );
    }
}
