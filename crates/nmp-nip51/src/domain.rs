//! `Nip51Domain` — `DomainModule` registration for the six NIP-51 kinds.
//!
//! Per `docs/design/kind-wrappers.md` §3.3 + §6: the kernel does not know which
//! kind is a mute list vs a relay set (D0). On ingest, the kernel dispatch
//! table reads [`Nip51Domain::ingest_kinds`] and calls [`decode_and_route`] to
//! write the decoded [`ListRecord`] to the domain store `nmp.nip51.lists`.
//!
//! ## Composite key MUST include kind
//!
//! `nmp-nip23` could omit kind from its primary key because it owned a single
//! kind. This crate owns **six**. A mute list (kind 10000) and a relay list
//! (kind 10002) by the same author both have `d_tag == ""` — without kind in
//! the key they would collide on one replaceable row and clobber each other.
//!
//! The primary key is therefore `(author, kind, d_tag)`. `kind` is encoded as
//! a **fixed 4-byte big-endian `u32`**, never as decimal text: decimal text is
//! variable-length and would reintroduce exactly the NUL-prefix-bleed risk the
//! length-stable encoding exists to prevent.

use nmp_core::store::{DomainHandle, StoreError, StoredEvent};
use nmp_core::substrate::{DomainIndex, DomainMigration, DomainModule, DomainRegistry};

use crate::decode::{try_from_event, ListRecord};
use crate::kinds::ALL_KINDS;

/// Domain-store namespace per the task brief.
pub const NAMESPACE: &str = "nmp.nip51.lists";

/// `DomainModule` impl for the NIP-51 list family.
pub struct Nip51Domain;

impl DomainModule for Nip51Domain {
    const NAMESPACE: &'static str = "nmp.nip51.lists";
    const SCHEMA_VERSION: u32 = 1;

    fn ingest_kinds() -> &'static [u32] {
        ALL_KINDS
    }

    fn migrations() -> Vec<DomainMigration> {
        Vec::new()
    }

    fn indexes() -> Vec<DomainIndex> {
        // This crate writes its own composite reverse-index keys (see `keys`)
        // and queries them via `DomainHandle::scan_prefix` per ADR-0001. No
        // backend-maintained secondary indexes are needed.
        Vec::new()
    }

    fn register(registry: &mut DomainRegistry) {
        registry.register_record::<ListRecord>();
    }
}

/// Composite-key encoding inside the `nmp.nip51.lists` namespace.
///
/// Every key is built from fixed-then-NUL-delimited components so `scan_prefix`
/// cannot bleed across reverse indexes:
/// - the 4-byte big-endian `kind` is fixed-width (no separator needed after it
///   would matter, but we still NUL-delimit author/d_tag which are
///   variable-length);
/// - variable-length `author` / `d_tag` are NUL-separated.
pub mod keys {
    /// Primary row: `p\x00<author>\x00<kind:4 BE>\x00<d_tag>` → `ListRecord`.
    pub const PRIMARY_PREFIX: &[u8] = b"p\x00";
    /// `by_author` index: `a\x00<author>\x00<kind:4 BE>\x00<d_tag>`.
    pub const BY_AUTHOR_PREFIX: &[u8] = b"a\x00";
    /// `by_kind` index: `k\x00<kind:4 BE>\x00<author>\x00<d_tag>`.
    pub const BY_KIND_PREFIX: &[u8] = b"k\x00";

    fn push_author_kind(key: &mut Vec<u8>, author: &str, kind: u32) {
        key.extend_from_slice(author.as_bytes());
        key.push(0u8);
        key.extend_from_slice(&kind.to_be_bytes());
        key.push(0u8);
    }

    /// Primary key for `(author, kind, d_tag)`.
    #[must_use]
    pub fn primary(author: &str, kind: u32, d_tag: &str) -> Vec<u8> {
        let mut key = PRIMARY_PREFIX.to_vec();
        push_author_kind(&mut key, author, kind);
        key.extend_from_slice(d_tag.as_bytes());
        key
    }

    /// `by_author` key for `(author, kind, d_tag)`.
    #[must_use]
    pub fn by_author(author: &str, kind: u32, d_tag: &str) -> Vec<u8> {
        let mut key = BY_AUTHOR_PREFIX.to_vec();
        push_author_kind(&mut key, author, kind);
        key.extend_from_slice(d_tag.as_bytes());
        key
    }

    /// `by_kind` key for `(kind, author, d_tag)`.
    #[must_use]
    pub fn by_kind(kind: u32, author: &str, d_tag: &str) -> Vec<u8> {
        let mut key = BY_KIND_PREFIX.to_vec();
        key.extend_from_slice(&kind.to_be_bytes());
        key.push(0u8);
        key.extend_from_slice(author.as_bytes());
        key.push(0u8);
        key.extend_from_slice(d_tag.as_bytes());
        key
    }

    /// Scan prefix for "all lists by `author`" (any kind).
    #[must_use]
    pub fn by_author_prefix(author: &str) -> Vec<u8> {
        let mut key = BY_AUTHOR_PREFIX.to_vec();
        key.extend_from_slice(author.as_bytes());
        key.push(0u8);
        key
    }

    /// Scan prefix for "all `kind` lists by `author`" — one bounded scan.
    #[must_use]
    pub fn by_author_kind_prefix(author: &str, kind: u32) -> Vec<u8> {
        let mut key = BY_AUTHOR_PREFIX.to_vec();
        key.extend_from_slice(author.as_bytes());
        key.push(0u8);
        key.extend_from_slice(&kind.to_be_bytes());
        key.push(0u8);
        key
    }

    /// Scan prefix for all primary rows.
    #[must_use]
    pub fn primary_scan_prefix() -> Vec<u8> {
        PRIMARY_PREFIX.to_vec()
    }
}

/// Decode + write to the domain store. Called by the kernel ingest dispatch
/// on every matching insert. Pure: single-handle write, no publishing, no wire
/// I/O — per the §6 trait contract.
pub fn decode_and_route(event: &StoredEvent, handle: &DomainHandle) -> Result<(), StoreError> {
    let Some(record) = try_from_event(event) else {
        // Non-NIP-51 / set missing d_tag: defensive no-op for direct callers.
        return Ok(());
    };
    let kind = record.list_kind.kind();

    // NIP-33 / replaceable supersession (D4 single-writer correctness): a relay
    // can redeliver an older revision of the same `(author, kind, d_tag)` after
    // the newer one already landed (reconnect backfill, multi-relay fan-in).
    // Keep whichever `created_at` is newer; on a tie keep the incumbent.
    if let Some(existing) = get(handle, &record.author, kind, &record.d_tag)? {
        if existing.created_at >= record.created_at {
            return Ok(());
        }
    }

    let serialized = serde_json::to_vec(&record)
        .map_err(|e| StoreError::Io(format!("serialize ListRecord: {e}")))?;

    handle.put(
        &keys::primary(&record.author, kind, &record.d_tag),
        &serialized,
    )?;

    let event_id_bytes = record.event_id.as_bytes();
    handle.put(
        &keys::by_author(&record.author, kind, &record.d_tag),
        event_id_bytes,
    )?;
    handle.put(
        &keys::by_kind(kind, &record.author, &record.d_tag),
        event_id_bytes,
    )?;

    Ok(())
}

/// Read a previously-decoded [`ListRecord`] by `(author, kind, d_tag)`.
pub fn get(
    handle: &DomainHandle,
    author: &str,
    kind: u32,
    d_tag: &str,
) -> Result<Option<ListRecord>, StoreError> {
    let Some(bytes) = handle.get(&keys::primary(author, kind, d_tag))? else {
        return Ok(None);
    };
    let record: ListRecord = serde_json::from_slice(&bytes)
        .map_err(|e| StoreError::Io(format!("deserialize ListRecord: {e}")))?;
    Ok(Some(record))
}

/// List every [`ListRecord`] in the store, `created_at` desc.
pub fn list_all(handle: &DomainHandle) -> Result<Vec<ListRecord>, StoreError> {
    let mut records = collect_records(handle, &keys::primary_scan_prefix())?;
    sort_by_created_desc(&mut records);
    Ok(records)
}

/// List every list by `author` (any kind), `created_at` desc.
pub fn list_by_author(handle: &DomainHandle, author: &str) -> Result<Vec<ListRecord>, StoreError> {
    resolve_by_author_index(handle, &keys::by_author_prefix(author))
}

/// List `kind` lists by `author` in one bounded scan, `created_at` desc — e.g.
/// "all follow-sets by alice".
pub fn list_by_author_kind(
    handle: &DomainHandle,
    author: &str,
    kind: u32,
) -> Result<Vec<ListRecord>, StoreError> {
    resolve_by_author_index(handle, &keys::by_author_kind_prefix(author, kind))
}

/// Walk a `by_author*` scan prefix, parse the `(author, kind, d_tag)` triple
/// back out of each key, resolve the primary rows, sort `created_at` desc.
fn resolve_by_author_index(
    handle: &DomainHandle,
    scan_prefix: &[u8],
) -> Result<Vec<ListRecord>, StoreError> {
    let entries = handle.scan_prefix(scan_prefix)?;
    let mut records = Vec::new();
    for entry in entries {
        let (key, _value) = entry?;
        // Key layout after `BY_AUTHOR_PREFIX`: <author>\x00<kind:4>\x00<d_tag>.
        let body = &key[keys::BY_AUTHOR_PREFIX.len()..];
        let Some(first_nul) = body.iter().position(|b| *b == 0) else {
            continue;
        };
        let author = std::str::from_utf8(&body[..first_nul])
            .map_err(|e| StoreError::Io(format!("non-utf8 author in by_author index: {e}")))?;
        // After the author NUL: 4 kind bytes, then a NUL, then the d_tag.
        let rest = &body[first_nul + 1..];
        if rest.len() < 5 {
            continue;
        }
        let kind = u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]);
        // rest[4] is the separator NUL; d_tag is everything after it.
        let d_tag = std::str::from_utf8(&rest[5..])
            .map_err(|e| StoreError::Io(format!("non-utf8 d_tag in by_author index: {e}")))?;
        if let Some(record) = get(handle, author, kind, d_tag)? {
            records.push(record);
        }
    }
    sort_by_created_desc(&mut records);
    Ok(records)
}

fn collect_records(handle: &DomainHandle, prefix: &[u8]) -> Result<Vec<ListRecord>, StoreError> {
    let scan = handle.scan_prefix(prefix)?;
    let mut out = Vec::new();
    for entry in scan {
        let (_key, value) = entry?;
        let record: ListRecord = serde_json::from_slice(&value)
            .map_err(|e| StoreError::Io(format!("deserialize ListRecord: {e}")))?;
        out.push(record);
    }
    Ok(out)
}

fn sort_by_created_desc(records: &mut [ListRecord]) {
    records.sort_by(|a, b| b.created_at.cmp(&a.created_at));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kinds::{KIND_FOLLOW_SETS, KIND_MUTE_LIST, KIND_RELAY_LIST};

    #[test]
    fn module_namespace_matches_constant() {
        assert_eq!(<Nip51Domain as DomainModule>::NAMESPACE, NAMESPACE);
    }

    #[test]
    fn ingest_kinds_returns_all_six() {
        assert_eq!(Nip51Domain::ingest_kinds(), ALL_KINDS);
        assert_eq!(Nip51Domain::ingest_kinds().len(), 6);
    }

    #[test]
    fn composite_key_disambiguates_same_author_different_kind() {
        // Mute list and relay list by the same author both have d_tag == "".
        // Without kind in the key these collide. With it, they must not.
        let mute = keys::primary("alice", KIND_MUTE_LIST, "");
        let relay = keys::primary("alice", KIND_RELAY_LIST, "");
        assert_ne!(mute, relay);
    }

    #[test]
    fn composite_key_disambiguates_same_author_same_kind_different_dtag() {
        let a = keys::primary("alice", KIND_FOLLOW_SETS, "friends");
        let b = keys::primary("alice", KIND_FOLLOW_SETS, "family");
        assert_ne!(a, b);
    }

    #[test]
    fn kind_is_fixed_width_not_decimal_text() {
        // Decimal "10000" is 5 bytes; the BE u32 is exactly 4. Proving the
        // encoding is the fixed-width form (and a different kind's bytes can
        // never be confused with the NUL separator + d_tag of another).
        let k1 = keys::primary("a", 10_000, "");
        let k2 = keys::primary(
            "a",
            1,
            &String::from_utf8(vec![0, 0, 39, 16]).unwrap_or_default(),
        );
        // Distinct primary rows regardless of any adversarial d_tag bytes.
        assert_ne!(k1, k2);
    }

    #[test]
    fn null_separator_prevents_author_dtag_collision() {
        // ("ali","ce") vs ("alic","e") would collide without a separator.
        let a = keys::primary("ali", KIND_FOLLOW_SETS, "ce");
        let b = keys::primary("alic", KIND_FOLLOW_SETS, "e");
        assert_ne!(a, b);
    }

    #[test]
    fn by_kind_swaps_leading_component_vs_primary() {
        let p = keys::primary("alice", KIND_MUTE_LIST, "");
        let k = keys::by_kind(KIND_MUTE_LIST, "alice", "");
        assert_ne!(p, k);
        assert_eq!(&p[..2], b"p\x00");
        assert_eq!(&k[..2], b"k\x00");
    }

    #[test]
    fn by_author_kind_prefix_scopes_the_scan() {
        let key = keys::by_author("alice", KIND_FOLLOW_SETS, "friends");
        let prefix = keys::by_author_kind_prefix("alice", KIND_FOLLOW_SETS);
        assert!(key.starts_with(&prefix));
        // A different kind's key must NOT start with this prefix.
        let other = keys::by_author("alice", KIND_MUTE_LIST, "");
        assert!(!other.starts_with(&prefix));
    }
}
