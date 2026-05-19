//! `ArticlesDomain` — `DomainModule` registration for kind:30023.
//!
//! Per `docs/design/kind-wrappers.md` §3.3 + §6: the kernel does not know
//! `kind 30023 == article` (D0). On ingest, the kernel's dispatch table (Phase
//! 1 §8) will read `ArticlesDomain::ingest_kinds()` and call `decode_and_route`
//! to write the decoded `ArticleRecord` to the domain store
//! `nmp.nip23.articles`. Until the kernel dispatch table lands, the
//! `decode_and_route` free function is callable directly — exercised by the
//! integration tests to prove the contract end-to-end.
//!
//! Per PD-008: decoded records are cached in the domain store **at ingest
//! time** (not on-demand). Reads query the store directly via the reverse
//! indexes documented below — never re-decode.

use nmp_core::store::{DomainHandle, StoreError, StoredEvent};
use nmp_core::substrate::{DomainIndex, DomainMigration, DomainModule, DomainRegistry};

use crate::decode::{try_from_event, ArticleRecord};
use crate::kinds::KIND_LONG_FORM_ARTICLE;

/// Domain-store namespace per the task brief: `nmp.nip23.articles`.
pub const NAMESPACE: &str = "nmp.nip23.articles";

/// Static slice the trait method returns — `&'static [u32]` cannot be built
/// from a const expression inline without this binding.
const INGEST_KINDS: &[u32] = &[KIND_LONG_FORM_ARTICLE];

/// `DomainModule` impl for NIP-23 articles.
pub struct ArticlesDomain;

impl DomainModule for ArticlesDomain {
    const NAMESPACE: &'static str = "nmp.nip23.articles";
    const SCHEMA_VERSION: u32 = 1;

    fn ingest_kinds() -> &'static [u32] {
        INGEST_KINDS
    }

    fn migrations() -> Vec<DomainMigration> {
        Vec::new()
    }

    fn indexes() -> Vec<DomainIndex> {
        // Reverse indexes (by_author / by_d_tag) are materialised via the
        // composite-key encoding in `keys::*` — see `decode_and_route`.
        // The `DomainIndex` registration surface is reserved for secondary
        // indexes the storage backend itself maintains; this crate writes
        // its own composite keys per ADR-0001 and queries them via
        // `DomainHandle::scan_prefix`. No backend index registrations needed.
        Vec::new()
    }

    fn register(registry: &mut DomainRegistry) {
        registry.register_record::<ArticleRecord>();
    }
}

/// Key prefixes inside the `nmp.nip23.articles` namespace. All keys are
/// length-prefixed where the prefix is variable so `scan_prefix` cannot bleed
/// across reverse indexes.
pub mod keys {
    /// Primary row: `p\x00<author>\x00<d_tag>` → `serde_json(ArticleRecord)`.
    pub const PRIMARY_PREFIX: &[u8] = b"p\x00";

    /// `by_author` reverse index: `a\x00<author>\x00<d_tag>` → `event_id`.
    pub const BY_AUTHOR_PREFIX: &[u8] = b"a\x00";

    /// `by_d_tag` reverse index: `d\x00<d_tag>\x00<author>` → `event_id`.
    /// (Articles share `d_tag`s across authors; the tail is author so a
    /// `scan_prefix(b"d\x00<dtag>\x00")` yields all authors with that d_tag.)
    pub const BY_D_TAG_PREFIX: &[u8] = b"d\x00";

    /// Compose the primary key.
    pub fn primary(author: &str, d_tag: &str) -> Vec<u8> {
        let mut key = PRIMARY_PREFIX.to_vec();
        key.extend_from_slice(author.as_bytes());
        key.push(0u8);
        key.extend_from_slice(d_tag.as_bytes());
        key
    }

    /// Compose the `by_author` key.
    pub fn by_author(author: &str, d_tag: &str) -> Vec<u8> {
        let mut key = BY_AUTHOR_PREFIX.to_vec();
        key.extend_from_slice(author.as_bytes());
        key.push(0u8);
        key.extend_from_slice(d_tag.as_bytes());
        key
    }

    /// Compose the `by_d_tag` key for a specific `(d_tag, author)` pair.
    pub fn by_d_tag(d_tag: &str, author: &str) -> Vec<u8> {
        let mut key = BY_D_TAG_PREFIX.to_vec();
        key.extend_from_slice(d_tag.as_bytes());
        key.push(0u8);
        key.extend_from_slice(author.as_bytes());
        key
    }

    /// `by_author` scan prefix: every article by `author`.
    pub fn by_author_prefix(author: &str) -> Vec<u8> {
        let mut key = BY_AUTHOR_PREFIX.to_vec();
        key.extend_from_slice(author.as_bytes());
        key.push(0u8);
        key
    }

    /// Scan-prefix variant for "all primary rows".
    pub fn primary_scan_prefix() -> Vec<u8> {
        PRIMARY_PREFIX.to_vec()
    }
}

/// Decode + write to the domain store. Called by the kernel ingest dispatch
/// (Phase 1) on every kind:30023 insert. Pure: single-handle write, no
/// publishing, no wire I/O — per the §6 trait contract.
pub fn decode_and_route(event: &StoredEvent, handle: &DomainHandle) -> Result<(), StoreError> {
    let Some(record) = try_from_event(event) else {
        // Non-30023 / missing d_tag: silently skip. The kernel dispatch table
        // is responsible for kind filtering; this is a defensive no-op for
        // direct callers (tests, app code that wants a single entry point).
        return Ok(());
    };

    // NIP-33 replaceable semantics (D4 single-writer correctness): a relay can
    // redeliver an older revision of the same `(author, d_tag)` after the newer
    // one already landed (reconnect backfill, multi-relay fan-in). Writing it
    // unconditionally would clobber the current record with stale data. Keep
    // whichever `created_at` is newer; on a tie we keep the incumbent (the
    // store's event-id tie-break refinement is tracked separately as a
    // backlog item — codex review #4 — and intentionally not duplicated here).
    if let Some(existing) = get(handle, &record.author, &record.d_tag)? {
        if existing.created_at >= record.created_at {
            return Ok(());
        }
    }

    let serialized = serde_json::to_vec(&record)
        .map_err(|e| StoreError::Io(format!("serialize ArticleRecord: {e}")))?;

    // Primary row.
    handle.put(&keys::primary(&record.author, &record.d_tag), &serialized)?;

    // Reverse indexes carry the event id so consumers can confirm a stale
    // index entry refers to a still-present primary row.
    let event_id_bytes = record.event_id.as_bytes();
    handle.put(&keys::by_author(&record.author, &record.d_tag), event_id_bytes)?;
    handle.put(&keys::by_d_tag(&record.d_tag, &record.author), event_id_bytes)?;

    Ok(())
}

/// Read a previously-decoded `ArticleRecord` by `(author, d_tag)`.
pub fn get(handle: &DomainHandle, author: &str, d_tag: &str) -> Result<Option<ArticleRecord>, StoreError> {
    let Some(bytes) = handle.get(&keys::primary(author, d_tag))? else {
        return Ok(None);
    };
    let record: ArticleRecord = serde_json::from_slice(&bytes)
        .map_err(|e| StoreError::Io(format!("deserialize ArticleRecord: {e}")))?;
    Ok(Some(record))
}

/// List all `ArticleRecord`s currently in the domain store, sorted by
/// `published_at` descending (NIP-23's intended display order — falls back
/// to `created_at` when `published_at` is absent).
pub fn list_all(handle: &DomainHandle) -> Result<Vec<ArticleRecord>, StoreError> {
    let mut records = collect_records(handle, &keys::primary_scan_prefix())?;
    sort_by_published_desc(&mut records);
    Ok(records)
}

/// List articles by a specific author, sorted by `published_at` desc.
pub fn list_by_author(handle: &DomainHandle, author: &str) -> Result<Vec<ArticleRecord>, StoreError> {
    // The `by_author` index is the small one. Walk it, then resolve primary
    // rows. For a memory backend this is two scans; for LMDB the `by_author`
    // walk is bounded by the per-author article count rather than the global
    // article count.
    let scan_prefix = keys::by_author_prefix(author);
    let entries = handle.scan_prefix(&scan_prefix)?;
    let mut records = Vec::new();
    for entry in entries {
        let (key, _value) = entry?;
        // The key tail (after `a\x00<author>\x00`) is the `d_tag`.
        let header_len = scan_prefix.len();
        if key.len() <= header_len {
            continue;
        }
        let d_tag_bytes = &key[header_len..];
        let d_tag = std::str::from_utf8(d_tag_bytes)
            .map_err(|e| StoreError::Io(format!("non-utf8 d_tag in by_author index: {e}")))?;
        if let Some(record) = get(handle, author, d_tag)? {
            records.push(record);
        }
    }
    sort_by_published_desc(&mut records);
    Ok(records)
}

fn collect_records(handle: &DomainHandle, prefix: &[u8]) -> Result<Vec<ArticleRecord>, StoreError> {
    let scan = handle.scan_prefix(prefix)?;
    let mut out = Vec::new();
    for entry in scan {
        let (_key, value) = entry?;
        let record: ArticleRecord = serde_json::from_slice(&value)
            .map_err(|e| StoreError::Io(format!("deserialize ArticleRecord: {e}")))?;
        out.push(record);
    }
    Ok(out)
}

fn sort_by_published_desc(records: &mut [ArticleRecord]) {
    records.sort_by(|a, b| {
        let a_ts = a.published_at.unwrap_or(a.created_at);
        let b_ts = b.published_at.unwrap_or(b.created_at);
        b_ts.cmp(&a_ts)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_namespace_matches_constant() {
        assert_eq!(<ArticlesDomain as DomainModule>::NAMESPACE, NAMESPACE);
    }

    #[test]
    fn module_ingest_kinds_returns_30023_only() {
        assert_eq!(ArticlesDomain::ingest_kinds(), &[KIND_LONG_FORM_ARTICLE]);
    }

    #[test]
    fn keys_primary_disambiguates_authors_with_same_d_tag() {
        let k1 = keys::primary("alice", "intro");
        let k2 = keys::primary("bob", "intro");
        assert_ne!(k1, k2);
    }

    #[test]
    fn keys_by_author_prefix_scopes_to_that_author() {
        let alice_key = keys::by_author("alice", "intro");
        let alice_prefix = keys::by_author_prefix("alice");
        assert!(alice_key.starts_with(&alice_prefix));
    }

    #[test]
    fn keys_by_d_tag_swaps_order_vs_primary() {
        let primary = keys::primary("alice", "intro");
        let by_d_tag = keys::by_d_tag("intro", "alice");
        // Same components but distinct prefixes prevent prefix-scan bleed.
        assert_ne!(primary, by_d_tag);
        assert_eq!(&primary[..2], b"p\x00");
        assert_eq!(&by_d_tag[..2], b"d\x00");
    }

    #[test]
    fn keys_use_null_separator_to_avoid_ambiguity() {
        // "ali" + "ce" vs "alice" + "" would collide without a separator.
        let collide_a = keys::primary("alice", "ce");
        let collide_b = keys::primary("ali", "cece");
        assert_ne!(collide_a, collide_b);
    }
}
