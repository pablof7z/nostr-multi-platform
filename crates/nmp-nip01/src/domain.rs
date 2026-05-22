//! Domain-store reverse indexes for NIP-01 kind:1 replies (those carrying a
//! non-empty NIP-10 reply marker — thread roots are skipped).
//!
//! Per `docs/design/kind-wrappers.md` §6, callers (apps or
//! `KernelEventObserver` impls) dispatch kind:1 events to `decode_and_route`;
//! we write a single reverse-index entry under the parent event id so the
//! read path can enumerate direct replies without re-scanning the event store.

use nmp_core::store::{DomainHandle, StoreError, StoredEvent};

use crate::decode::try_from_event;

/// Domain-store namespace.
pub const NAMESPACE: &str = "nmp.nip01.replies";

pub mod keys {
    //! Composite-key encoding for the `nmp.nip01.replies` namespace. The
    //! `parent_id` is fixed-length hex (64 chars) so a separator is not
    //! strictly required, but we keep the byte form consistent with
    //! `nmp-nip23` for grep-ability.

    /// Reverse index: `r\x00<parent_id>\x00<reply_id>` → empty value (the
    /// reply event id is encoded in the key so a single key-only scan
    /// enumerates direct replies).
    pub const BY_PARENT_PREFIX: &[u8] = b"r\x00";

    pub fn by_parent(parent_id: &str, reply_id: &str) -> Vec<u8> {
        let mut key = BY_PARENT_PREFIX.to_vec();
        key.extend_from_slice(parent_id.as_bytes());
        key.push(0u8);
        key.extend_from_slice(reply_id.as_bytes());
        key
    }

    pub fn by_parent_prefix(parent_id: &str) -> Vec<u8> {
        let mut key = BY_PARENT_PREFIX.to_vec();
        key.extend_from_slice(parent_id.as_bytes());
        key.push(0u8);
        key
    }
}

/// Decode + write to the domain store. Skips events that aren't kind:1, and
/// kind:1 events that have no NIP-10 reply pointer (those are thread roots).
pub fn decode_and_route(event: &StoredEvent, handle: &DomainHandle) -> Result<(), StoreError> {
    let Some(record) = try_from_event(event) else {
        return Ok(());
    };
    let Some(parent) = record.refs.reply.as_ref() else {
        return Ok(()); // Thread root, not a reply — nothing to index.
    };

    let key = keys::by_parent(&parent.id, &record.event_id);
    handle.put(&key, &[])?;
    Ok(())
}

/// Enumerate the reply event ids registered under `parent_id` (insertion order
/// per the backend's scan_prefix contract).
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
        let reply_id_bytes = &key[header_len..];
        let reply_id = std::str::from_utf8(reply_id_bytes)
            .map_err(|e| StoreError::Io(format!("non-utf8 reply id in by_parent index: {e}")))?;
        ids.push(reply_id.to_string());
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_by_parent_is_prefixed_by_by_parent_prefix() {
        let k = keys::by_parent("PARENT", "REPLY");
        let p = keys::by_parent_prefix("PARENT");
        assert!(k.starts_with(&p));
    }

    #[test]
    fn keys_distinct_parents_do_not_alias() {
        let k1 = keys::by_parent("alice", "ce");
        let k2 = keys::by_parent("ali", "cece");
        assert_ne!(k1, k2);
    }
}
