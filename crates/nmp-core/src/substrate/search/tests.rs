//! Tests for the crate-registered search-scope registry (#1811).

use std::collections::BTreeSet;
use std::sync::Arc;

use super::*;
use crate::store::{MemEventStore, RawEvent, SearchScopeId, StoredEvent, VerifiedEvent};
use nmp_store::EventStore;

/// A `#[cfg(test)]` fixture scope over a test kind (40001) indexing the event
/// `.content`. NOT a production scope — proves the registry without a real NIP.
struct FixtureNoteScope {
    scope: SearchScopeId,
    privacy: SearchPrivacyPolicy,
}

impl FixtureNoteScope {
    fn new(label: &'static str, privacy: SearchPrivacyPolicy) -> Arc<Self> {
        Arc::new(Self {
            scope: SearchScopeId::from_label(label),
            privacy,
        })
    }
}

impl SearchScopeProvider for FixtureNoteScope {
    fn spec(&self) -> SearchIndexSpec {
        SearchIndexSpec {
            scope: self.scope,
            source: "test-fixture-note",
            kinds: BTreeSet::from([40001]),
            fields: vec![SearchField::new(0)],
            privacy: self.privacy,
            cache_mode: CacheSearchMode::Both,
        }
    }

    fn extract(&self, event: &StoredEvent) -> Vec<(SearchField, String)> {
        vec![(SearchField::new(0), event.raw.content.clone())]
    }
}

fn stored(id: &str, kind: u32, content: &str, created_at: u64) -> StoredEvent {
    let raw = RawEvent {
        id: id.to_string(),
        pubkey: "11".repeat(32),
        created_at,
        kind,
        tags: Vec::new(),
        content: content.to_string(),
        sig: "22".repeat(64),
    };
    let _ = VerifiedEvent::from_raw_unchecked(raw.clone());
    StoredEvent {
        raw: std::sync::Arc::new(raw),
        received_at_ms: created_at * 1000,
    }
}

#[test]
fn register_then_compile_yields_one_spec() {
    let reg = SearchScopeRegistry::new();
    let d = reg.register(FixtureNoteScope::new(
        "test.note",
        SearchPrivacyPolicy::PublicIndexable,
    ));
    assert_eq!(d, SearchScopeDisposition::Installed);
    let compiled = reg.compile();
    assert_eq!(compiled.len(), 1);
    assert!(compiled[0].kinds.contains(&40001));
    assert!(!compiled[0].local_only_private);
}

#[test]
fn duplicate_scope_yields() {
    let reg = SearchScopeRegistry::new();
    let a = reg.register(FixtureNoteScope::new(
        "test.dup",
        SearchPrivacyPolicy::PublicIndexable,
    ));
    let b = reg.register(FixtureNoteScope::new(
        "test.dup",
        SearchPrivacyPolicy::PublicIndexable,
    ));
    assert_eq!(a, SearchScopeDisposition::Installed);
    assert_eq!(b, SearchScopeDisposition::YieldedToExisting);
    assert_eq!(reg.len(), 1);
}

#[test]
fn local_only_private_scope_dropped_from_compile() {
    let reg = SearchScopeRegistry::new();
    reg.register(FixtureNoteScope::new(
        "test.private",
        SearchPrivacyPolicy::LocalOnlyPrivate,
    ));
    assert_eq!(reg.len(), 1, "registered");
    assert!(
        reg.compile().is_empty(),
        "but compiled away from the public index"
    );
}

#[test]
fn install_into_then_search_matches_token_and_prefix() {
    let reg = SearchScopeRegistry::new();
    let scope = SearchScopeId::from_label("test.note");
    reg.register(FixtureNoteScope::new(
        "test.note",
        SearchPrivacyPolicy::PublicIndexable,
    ));

    let store = MemEventStore::new();
    // Install BEFORE ingest so the index is maintained on insert.
    reg.install_into(&store);

    store
        .insert(
            VerifiedEvent::from_raw_unchecked(
                stored(
                    "aa".repeat(32).as_str(),
                    40001,
                    "hello satoshi nakamoto",
                    100,
                )
                .raw
                .as_ref()
                .clone(),
            ),
            &"wss://r/".to_string(),
            100_000,
        )
        .unwrap();

    // Prefix match: "sato" → "satoshi".
    let mut hits = Vec::new();
    let status = store
        .text_search_visit(
            &nmp_store::TextSearchQuery {
                scope,
                query: "sato".into(),
                kinds: BTreeSet::new(),
                since: None,
                until: None,
                limit: 10,
                order: nmp_store::TextSearchOrder::NewestFirst,
                budget: nmp_store::TextSearchBudget::default(),
            },
            &mut |hit| {
                hits.push(hit);
                std::ops::ControlFlow::Continue(())
            },
        )
        .unwrap();
    assert_eq!(status, nmp_store::TextSearchStatus::Complete);
    assert_eq!(hits.len(), 1, "prefix 'sato' matches token 'satoshi'");
}
