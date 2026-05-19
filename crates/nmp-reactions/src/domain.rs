//! `ReactionsDomain` — `DomainModule` registration for kinds 7 / 6 / 16.
//!
//! Per `docs/design/kind-wrappers.md` §3.3 + §6: the kernel does not know
//! `kind 7 == reaction` (D0). On ingest, the kernel's dispatch table (Phase 1
//! §8) reads `ReactionsDomain::ingest_kinds()` and calls `decode_and_route` to
//! write the decoded [`SocialRecord`] to the domain store. Until the kernel
//! dispatch table lands, `decode_and_route` is callable directly — exercised by
//! the integration tests to prove the contract end-to-end.
//!
//! ## Not replaceable — idempotency on `event_id`
//!
//! Kinds 7 / 6 / 16 are **regular events**, NOT replaceable. NIP-33-style
//! `(author, d_tag)` supersession does not apply. The nip23 "stale redelivery"
//! guard maps here to plain **duplicate-`event_id` idempotency**: the primary
//! key is the reaction's own immutable `event_id`, so re-ingesting the same
//! `id` overwrites an identical row — never double-counts downstream. The
//! domain accepts every *distinct* event (this is not a D4 violation: the
//! single writer is the actor; the domain is a derived cache).
//!
//! Per PD-008 decoded records are cached at ingest time; reads query the
//! composite reverse indexes below, never re-decode.

use nmp_core::planner::NaddrCoord;
use nmp_core::store::{DomainHandle, StoreError, StoredEvent};
use nmp_core::substrate::{DomainIndex, DomainMigration, DomainModule};

use crate::decode::{try_from_event, ReactionTarget, SocialKind, SocialRecord};
use crate::kinds::SOCIAL_KINDS;

/// Domain-store namespace.
pub const NAMESPACE: &str = "nmp.reactions";

const INGEST_KINDS: &[u32] = SOCIAL_KINDS;

/// `DomainModule` impl for NIP-25 reactions + NIP-18 reposts.
pub struct ReactionsDomain;

impl DomainModule for ReactionsDomain {
    const NAMESPACE: &'static str = "nmp.reactions";
    const SCHEMA_VERSION: u32 = 1;

    fn ingest_kinds() -> &'static [u32] {
        INGEST_KINDS
    }

    fn migrations() -> Vec<DomainMigration> {
        Vec::new()
    }

    fn indexes() -> Vec<DomainIndex> {
        // Reverse indexes are materialised via the composite-key encoding in
        // `keys::*` (ADR-0001) and queried with `DomainHandle::scan_prefix`;
        // no backend-maintained secondary indexes are needed.
        Vec::new()
    }
}

/// Composite key encodings inside the `nmp.reactions` namespace.
///
/// Every key is NUL-separated and variant-tagged so `scan_prefix` cannot bleed
/// across indexes or across `ReactionTarget` variants. The `target_key`
/// encoding is the load-bearing one: an event-id target and an addressable
/// target must never collide, so each is prefixed by a distinct variant byte
/// and (for addresses) every component is NUL-delimited.
pub mod keys {
    use super::{NaddrCoord, ReactionTarget};

    /// Primary row: `r\x00<event_id>` → `serde_json(SocialRecord)`. The key is
    /// the reaction's own immutable id (regular event → id is the identity);
    /// re-ingesting the same id overwrites the identical row (idempotency).
    pub const PRIMARY_PREFIX: &[u8] = b"r\x00";

    /// `by_target`: `t\x00<target_key>\x00<content>\x00<reactor>` → `event_id`.
    /// A per-target aggregate is ONE bounded prefix scan on
    /// `t\x00<target_key>\x00`.
    pub const BY_TARGET_PREFIX: &[u8] = b"t\x00";

    /// `by_reactor`: `u\x00<reactor>\x00<target_key>` → `event_id`.
    pub const BY_REACTOR_PREFIX: &[u8] = b"u\x00";

    /// Encode a [`ReactionTarget`] unambiguously. Variant tag `E` = event id,
    /// `A` = address; address components are NUL-separated so
    /// `(kind, pubkey, "")` and `(kind, pubkey)` + an empty trailing d-tag stay
    /// distinct, and `("ali","ce")` cannot collide with `("alic","e")`.
    pub fn target_key(target: &ReactionTarget) -> Vec<u8> {
        let mut k = Vec::new();
        match target {
            ReactionTarget::Event(id) => {
                k.push(b'E');
                k.push(0u8);
                k.extend_from_slice(id.as_bytes());
            }
            ReactionTarget::Address(c) => {
                k.push(b'A');
                k.push(0u8);
                k.extend_from_slice(c.kind.to_string().as_bytes());
                k.push(0u8);
                k.extend_from_slice(c.pubkey.as_bytes());
                k.push(0u8);
                k.extend_from_slice(c.d_tag.as_bytes());
            }
        }
        k
    }

    /// Build an [`NaddrCoord`] target key without constructing the enum (used
    /// by `list_for_target` callers that already hold a coord).
    pub fn target_key_for_address(coord: &NaddrCoord) -> Vec<u8> {
        target_key(&ReactionTarget::Address(coord.clone()))
    }

    /// Primary key for an event id.
    pub fn primary(event_id: &str) -> Vec<u8> {
        let mut k = PRIMARY_PREFIX.to_vec();
        k.extend_from_slice(event_id.as_bytes());
        k
    }

    /// Scan prefix for "all primary rows".
    pub fn primary_scan_prefix() -> Vec<u8> {
        PRIMARY_PREFIX.to_vec()
    }

    /// `by_target` key: `t\x00<target_key>\x00<content>\x00<reactor>`.
    pub fn by_target(target: &ReactionTarget, content: &str, reactor: &str) -> Vec<u8> {
        let mut k = BY_TARGET_PREFIX.to_vec();
        k.extend_from_slice(&target_key(target));
        k.push(0u8);
        k.extend_from_slice(content.as_bytes());
        k.push(0u8);
        k.extend_from_slice(reactor.as_bytes());
        k
    }

    /// `by_target` scan prefix: every reaction/repost on `target`.
    pub fn by_target_prefix(target: &ReactionTarget) -> Vec<u8> {
        let mut k = BY_TARGET_PREFIX.to_vec();
        k.extend_from_slice(&target_key(target));
        k.push(0u8);
        k
    }

    /// `by_target_content` scan prefix: every reactor who used exactly
    /// `content` on `target` (e.g. "count of 👍 on event X").
    pub fn by_target_content_prefix(target: &ReactionTarget, content: &str) -> Vec<u8> {
        let mut k = BY_TARGET_PREFIX.to_vec();
        k.extend_from_slice(&target_key(target));
        k.push(0u8);
        k.extend_from_slice(content.as_bytes());
        k.push(0u8);
        k
    }

    /// `by_reactor` key: `u\x00<reactor>\x00<target_key>`.
    pub fn by_reactor(reactor: &str, target: &ReactionTarget) -> Vec<u8> {
        let mut k = BY_REACTOR_PREFIX.to_vec();
        k.extend_from_slice(reactor.as_bytes());
        k.push(0u8);
        k.extend_from_slice(&target_key(target));
        k
    }

    /// `by_reactor` scan prefix: every target a reactor touched.
    pub fn by_reactor_prefix(reactor: &str) -> Vec<u8> {
        let mut k = BY_REACTOR_PREFIX.to_vec();
        k.extend_from_slice(reactor.as_bytes());
        k.push(0u8);
        k
    }
}

/// The string a reaction/repost contributes to an aggregate. For kind:7 this is
/// the reaction `content` (`"+"`, `"-"`, an emoji, …). For reposts it is a
/// stable synthetic token so reposts aggregate together and never collide with
/// a literal reaction whose content happened to be the same text.
fn aggregate_content(record: &SocialRecord) -> String {
    match &record.kind {
        SocialKind::Reaction { content, .. } => content.clone(),
        SocialKind::Repost { .. } => "\x01repost".to_string(),
        SocialKind::GenericRepost { .. } => "\x01repost".to_string(),
    }
}

/// Decode + write to the domain store. Called by the kernel ingest dispatch
/// (Phase 1) on every kind:7/6/16 insert. Pure: single-handle write, no
/// publishing, no wire I/O.
///
/// Idempotency: the primary key is `record.event_id`. Re-ingesting the same id
/// rewrites the identical primary row and identical index rows — the count a
/// downstream summary derives is unchanged (not doubled).
pub fn decode_and_route(event: &StoredEvent, handle: &DomainHandle) -> Result<(), StoreError> {
    let Some(record) = try_from_event(event) else {
        // Non-{7,6,16}: defensive no-op (the kernel dispatch table is
        // responsible for kind filtering; direct callers get a single entry
        // point that simply ignores irrelevant kinds).
        return Ok(());
    };

    let serialized = serde_json::to_vec(&record)
        .map_err(|e| StoreError::Io(format!("serialize SocialRecord: {e}")))?;

    // Primary row keyed on the immutable event id (regular event identity).
    handle.put(&keys::primary(&record.event_id), &serialized)?;

    let content = aggregate_content(&record);
    let event_id_bytes = record.event_id.as_bytes();
    handle.put(
        &keys::by_target(&record.target, &content, &record.author),
        event_id_bytes,
    )?;
    handle.put(
        &keys::by_reactor(&record.author, &record.target),
        event_id_bytes,
    )?;

    Ok(())
}

/// Read a previously-decoded [`SocialRecord`] by its own event id.
pub fn get(handle: &DomainHandle, event_id: &str) -> Result<Option<SocialRecord>, StoreError> {
    let Some(bytes) = handle.get(&keys::primary(event_id))? else {
        return Ok(None);
    };
    let record: SocialRecord = serde_json::from_slice(&bytes)
        .map_err(|e| StoreError::Io(format!("deserialize SocialRecord: {e}")))?;
    Ok(Some(record))
}

/// Every reaction/repost on `target`, newest first (by `created_at` desc, then
/// `event_id` for determinism). Reposts are included; callers wanting only
/// reposts filter on [`SocialRecord::is_repost`].
pub fn list_for_target(
    handle: &DomainHandle,
    target: &ReactionTarget,
) -> Result<Vec<SocialRecord>, StoreError> {
    let prefix = keys::by_target_prefix(target);
    let mut records = resolve_index(handle, &prefix)?;
    sort_newest_first(&mut records);
    Ok(records)
}

/// Every reaction/repost authored by `reactor`, newest first.
pub fn list_by_reactor(
    handle: &DomainHandle,
    reactor: &str,
) -> Result<Vec<SocialRecord>, StoreError> {
    let prefix = keys::by_reactor_prefix(reactor);
    let mut records = resolve_index(handle, &prefix)?;
    sort_newest_first(&mut records);
    Ok(records)
}

/// Aggregate summary for one target: counts grouped by aggregate-content, with
/// per-`(reactor, target)` de-dupe keeping the reactor's NEWEST reaction
/// (standard client behaviour — a user switching 👍→❤️ counts once, as ❤️).
///
/// Returns `(content, count)` pairs sorted by count desc then content asc so
/// SwiftUI diffing is stable, plus the total distinct-reactor count.
pub fn reaction_summary(
    handle: &DomainHandle,
    target: &ReactionTarget,
) -> Result<ReactionSummary, StoreError> {
    use std::collections::BTreeMap;

    // Reactions only — reposts (kinds 6/16) are a *separate* surface
    // (`RepostsView` / `list_for_target` filtered to `is_repost`), never folded
    // into the reaction aggregate. This keeps the domain-side summary identical
    // to the view-side `ReactionAccumulator::reaction_summary`, which applies
    // the same filter.
    let records: Vec<SocialRecord> = list_for_target(handle, target)?
        .into_iter()
        .filter(SocialRecord::is_reaction)
        .collect();

    // Keep the newest record per reactor. `records` is already newest-first, so
    // the first time we see a reactor is its newest reaction.
    let mut newest_per_reactor: BTreeMap<String, SocialRecord> = BTreeMap::new();
    for r in records {
        newest_per_reactor.entry(r.author.clone()).or_insert(r);
    }

    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    for r in newest_per_reactor.values() {
        *counts.entry(aggregate_content(r)).or_insert(0) += 1;
    }

    let total: u64 = counts.values().sum();
    let mut entries: Vec<(String, u64)> = counts.into_iter().collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    Ok(ReactionSummary { entries, total })
}

/// Output of [`reaction_summary`]. Always renderable (D1): an empty `entries`
/// with `total == 0` is a valid summary, not an error or `Option::None`.
#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReactionSummary {
    /// `(aggregate_content, count)` sorted by count desc then content asc.
    /// Repost rows use the synthetic `"\x01repost"` content token.
    pub entries: Vec<(String, u64)>,
    /// Total distinct reactors (after per-reactor newest-wins collapse).
    pub total: u64,
}

fn resolve_index(handle: &DomainHandle, prefix: &[u8]) -> Result<Vec<SocialRecord>, StoreError> {
    let scan = handle.scan_prefix(prefix)?;
    let mut out = Vec::new();
    for entry in scan {
        let (_key, value) = entry?;
        let event_id = std::str::from_utf8(&value)
            .map_err(|e| StoreError::Io(format!("non-utf8 event id in index: {e}")))?;
        if let Some(record) = get(handle, event_id)? {
            out.push(record);
        }
    }
    Ok(out)
}

fn sort_newest_first(records: &mut [SocialRecord]) {
    records.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| a.event_id.cmp(&b.event_id))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::ReactionTarget;

    #[test]
    fn module_namespace_matches_constant() {
        assert_eq!(<ReactionsDomain as DomainModule>::NAMESPACE, NAMESPACE);
    }

    #[test]
    fn module_ingest_kinds_returns_7_6_16() {
        assert_eq!(ReactionsDomain::ingest_kinds(), &[7, 6, 16]);
    }

    #[test]
    fn target_key_disambiguates_event_id_vs_naddr() {
        let ev = ReactionTarget::Event("abc".into());
        let addr = ReactionTarget::Address(NaddrCoord {
            pubkey: "abc".into(),
            kind: 1,
            d_tag: String::new(),
        });
        assert_ne!(keys::target_key(&ev), keys::target_key(&addr));
        // Distinct variant tag byte.
        assert_eq!(keys::target_key(&ev)[0], b'E');
        assert_eq!(keys::target_key(&addr)[0], b'A');
    }

    #[test]
    fn target_key_address_components_nul_separated_no_collision() {
        // ("ali","ce") vs ("alic","e") must not collide.
        let a = ReactionTarget::Address(NaddrCoord {
            pubkey: "ali".into(),
            kind: 7,
            d_tag: "ce".into(),
        });
        let b = ReactionTarget::Address(NaddrCoord {
            pubkey: "alic".into(),
            kind: 7,
            d_tag: "e".into(),
        });
        assert_ne!(keys::target_key(&a), keys::target_key(&b));
    }

    #[test]
    fn by_target_prefix_is_a_prefix_of_by_target_key() {
        let target = ReactionTarget::Event("e1".into());
        let full = keys::by_target(&target, "+", "reactor1");
        let prefix = keys::by_target_prefix(&target);
        assert!(full.starts_with(&prefix));
    }

    #[test]
    fn by_target_content_prefix_scopes_to_content() {
        let target = ReactionTarget::Event("e1".into());
        let thumbs = keys::by_target(&target, "👍", "reactor1");
        let prefix = keys::by_target_content_prefix(&target, "👍");
        assert!(thumbs.starts_with(&prefix));
        // A different content must not match the prefix.
        let heart = keys::by_target(&target, "❤️", "reactor1");
        assert!(!heart.starts_with(&prefix));
    }

    #[test]
    fn by_reactor_prefix_scopes_to_reactor() {
        let target = ReactionTarget::Event("e1".into());
        let full = keys::by_reactor("alice", &target);
        let prefix = keys::by_reactor_prefix("alice");
        assert!(full.starts_with(&prefix));
        // "ali" + "ce..." must not collide with "alice" + "..." due to NUL.
        let other = keys::by_reactor("ali", &ReactionTarget::Event("ce-e1".into()));
        assert_ne!(full, other);
    }

    #[test]
    fn primary_key_is_the_event_id() {
        let k = keys::primary("deadbeef");
        assert!(k.starts_with(keys::PRIMARY_PREFIX));
        assert!(k.ends_with(b"deadbeef"));
    }
}
