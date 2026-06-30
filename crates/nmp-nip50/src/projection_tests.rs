//! Result-projection tests (#1811): cache hits via the FTS seam, and
//! first-arrival-wins dedupe between a cache hit and a relay hit.

use std::sync::Arc;

use nmp_core::substrate::{KernelEvent, SearchScopeProvider, SearchScopeRegistry};
use nmp_store::{EventStore, MemEventStore, RawEvent, VerifiedEvent};

use super::*;
use crate::request::{SearchScope, SearchTargets};
use crate::scopes::{NoteSearchScope, ProfileSearchScope};

const NOTE_KIND: u32 = 1;

fn note_id(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

fn raw_note(id_byte: u8, content: &str, created_at: u64) -> RawEvent {
    RawEvent {
        id: note_id(id_byte),
        pubkey: "01".repeat(32),
        created_at,
        kind: NOTE_KIND,
        tags: vec![],
        content: content.to_string(),
        sig: "a".repeat(128),
    }
}

/// A `MemEventStore` with the three NIP-50 scopes compiled + installed through
/// the real `nmp-core` registry (the same path the composition root uses).
fn store_with_scopes() -> MemEventStore {
    let registry = SearchScopeRegistry::new();
    registry.register(Arc::new(ProfileSearchScope::new()) as Arc<dyn SearchScopeProvider>);
    registry.register(Arc::new(NoteSearchScope::new()) as Arc<dyn SearchScopeProvider>);
    let store = MemEventStore::new();
    registry.install_into(&store);
    store
}

fn insert(store: &MemEventStore, raw: RawEvent) {
    let at = raw.created_at;
    store
        .insert(
            VerifiedEvent::from_raw_unchecked(raw),
            &"wss://cache/".to_string(),
            at,
        )
        .unwrap();
}

fn note_request(query: &str) -> SearchRequest {
    SearchRequest::new(
        query,
        SearchScope::Kinds(std::collections::BTreeSet::from([NOTE_KIND])),
        SearchTargets::UserPreferred,
        None,
    )
    .expect("query")
}

fn kernel_note(id_byte: u8, content: &str, created_at: u64, relay: &str) -> KernelEvent {
    KernelEvent {
        id: note_id(id_byte),
        author: "01".repeat(32),
        kind: NOTE_KIND,
        created_at,
        tags: vec![],
        content: content.to_string(),
        relay_provenance: vec![relay.to_string()],
    }
}

#[test]
fn cache_hit_surfaces_as_cache_source() {
    let store = store_with_scopes();
    insert(&store, raw_note(1, "hello nostr world", 100));
    insert(&store, raw_note(2, "totally unrelated", 101));

    let mut projection = SearchResultsProjection::new(note_request("nostr"));
    let status = projection.ingest_cache_from_store(&store);

    assert_eq!(status, nmp_store::TextSearchStatus::Complete);
    let snap = projection.snapshot();
    assert_eq!(snap.hits.len(), 1, "only the matching note is a hit");
    assert_eq!(snap.hits[0].id, note_id(1));
    assert_eq!(snap.hits[0].source, SearchHitSource::Cache);
    assert_eq!(snap.hits[0].content, "hello nostr world");
}

#[test]
fn cache_hit_matches_token_and_prefix() {
    let store = store_with_scopes();
    insert(&store, raw_note(1, "satoshi nakamoto", 100));

    // Prefix typeahead: "sato" matches the indexed "satoshi" token.
    let mut projection = SearchResultsProjection::new(note_request("sato"));
    projection.ingest_cache_from_store(&store);
    assert_eq!(projection.snapshot().hits.len(), 1);
}

#[test]
fn cache_then_relay_first_arrival_wins() {
    let store = store_with_scopes();
    insert(&store, raw_note(1, "nostr cache first", 100));

    let mut projection = SearchResultsProjection::new(note_request("nostr"));
    // Cache arrives first.
    projection.ingest_cache_from_store(&store);
    // Relay echoes the same event id later.
    projection.ingest_relay_event(
        &kernel_note(1, "nostr cache first", 100, "wss://relay/"),
        "wss://relay/".to_string(),
    );

    let snap = projection.snapshot();
    assert_eq!(snap.hits.len(), 1, "duplicate id deduped");
    assert_eq!(
        snap.hits[0].source,
        SearchHitSource::Cache,
        "first arrival wins"
    );
}

#[test]
fn relay_then_cache_first_arrival_wins() {
    let store = store_with_scopes();
    insert(&store, raw_note(1, "nostr relay first", 100));

    let mut projection = SearchResultsProjection::new(note_request("nostr"));
    // Relay arrives first.
    projection.ingest_relay_event(
        &kernel_note(1, "nostr relay first", 100, "wss://relay/"),
        "wss://relay/".to_string(),
    );
    // Cache fills later — must NOT overwrite the relay-sourced hit.
    projection.ingest_cache_from_store(&store);

    let snap = projection.snapshot();
    assert_eq!(snap.hits.len(), 1);
    assert_eq!(
        snap.hits[0].source,
        SearchHitSource::Relay("wss://relay/".to_string()),
        "first arrival (relay) wins"
    );
}

#[test]
fn unmapped_scope_reports_unsupported() {
    let store = store_with_scopes();
    // A multi-kind interest maps to no single FTS scope.
    let request = SearchRequest::new(
        "nostr",
        SearchScope::Kinds(std::collections::BTreeSet::from([1, 30_023])),
        SearchTargets::UserPreferred,
        None,
    )
    .expect("query");
    let mut projection = SearchResultsProjection::new(request);
    let status = projection.ingest_cache_from_store(&store);
    assert_eq!(status, nmp_store::TextSearchStatus::Unsupported);
    assert!(projection.snapshot().hits.is_empty());
}
