//! #1811 — in-memory full-text-search backend tests.
//!
//! Holds the `MemEventStore` to the FTS contract: token match, prefix
//! (typeahead) match, multi-token AND, newest-first ordering, limit/early-stop,
//! empty-query → Complete+empty, and delete cleanup (no stale hits survive
//! source removal). Uses a `#[cfg(test)]` fixture spec over a test kind — NOT a
//! production scope.

use std::collections::BTreeSet;
use std::ops::ControlFlow;
use std::sync::Arc;

use crate::text_search::{
    CompiledIndexSpec, ExtractFn, SearchField, SearchScopeId, TextSearchBudget, TextSearchHit,
    TextSearchOrder, TextSearchQuery, TextSearchStatus,
};
use crate::types::{DeleteFilter, RawEvent, StoredEvent, VerifiedEvent};
use crate::{EventStore, MemEventStore};

const TEST_KIND: u32 = 40001;
const TEST_LABEL: &str = "test.note";

fn scope_id() -> SearchScopeId {
    SearchScopeId::from_label(TEST_LABEL)
}

/// A fixture spec indexing the event `.content` for `TEST_KIND`.
fn fixture_spec() -> CompiledIndexSpec {
    let extract: Arc<ExtractFn> = Arc::new(|ev: &StoredEvent| {
        vec![(SearchField::new(0), ev.raw.content.clone())]
    });
    CompiledIndexSpec {
        scope_id: scope_id(),
        kinds: BTreeSet::from([TEST_KIND]),
        extract,
        local_only_private: false,
    }
}

fn make_event(id_byte: u8, content: &str, created_at: u64) -> RawEvent {
    RawEvent {
        id: format!("{id_byte:02x}").repeat(32),
        pubkey: "01".repeat(32),
        created_at,
        kind: TEST_KIND,
        tags: vec![],
        content: content.to_string(),
        sig: "a".repeat(128),
    }
}

fn store_with_index() -> MemEventStore {
    let store = MemEventStore::new();
    store.install_search_index_specs(vec![fixture_spec()]);
    store
}

fn insert(store: &MemEventStore, ev: RawEvent, at: u64) {
    store
        .insert(VerifiedEvent::from_raw_unchecked(ev), &"wss://r/".to_string(), at)
        .unwrap();
}

fn query(q: &str) -> TextSearchQuery {
    TextSearchQuery {
        scope: scope_id(),
        query: q.to_string(),
        kinds: BTreeSet::new(),
        since: None,
        until: None,
        limit: 50,
        order: TextSearchOrder::NewestFirst,
        budget: TextSearchBudget::default(),
    }
}

fn run(store: &MemEventStore, q: &TextSearchQuery) -> (Vec<TextSearchHit>, TextSearchStatus) {
    let mut hits = Vec::new();
    let status = store
        .text_search_visit(q, &mut |hit| {
            hits.push(hit);
            ControlFlow::Continue(())
        })
        .unwrap();
    (hits, status)
}

#[test]
fn token_match() {
    let store = store_with_index();
    insert(&store, make_event(0x01, "hello satoshi nakamoto", 100), 100_000);
    let (hits, status) = run(&store, &query("nakamoto"));
    assert_eq!(status, TextSearchStatus::Complete);
    assert_eq!(hits.len(), 1);
}

#[test]
fn prefix_match() {
    let store = store_with_index();
    insert(&store, make_event(0x01, "hello satoshi", 100), 100_000);
    // "sato" must match token "satoshi" (typeahead, trailing-token prefix).
    let (hits, status) = run(&store, &query("sato"));
    assert_eq!(status, TextSearchStatus::Complete);
    assert_eq!(hits.len(), 1);
    // A non-prefix returns nothing.
    let (none, _) = run(&store, &query("xyz"));
    assert!(none.is_empty());
}

#[test]
fn multi_token_and() {
    let store = store_with_index();
    insert(&store, make_event(0x01, "alpha beta gamma", 100), 100_000);
    insert(&store, make_event(0x02, "alpha delta", 101), 101_000);
    // Both "alpha" AND "beta*" must be present → only event 1.
    let (hits, status) = run(&store, &query("alpha bet"));
    assert_eq!(status, TextSearchStatus::Complete);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].created_at, 100);
}

#[test]
fn newest_first() {
    let store = store_with_index();
    insert(&store, make_event(0x01, "shared term", 100), 100_000);
    insert(&store, make_event(0x02, "shared term", 300), 300_000);
    insert(&store, make_event(0x03, "shared term", 200), 200_000);
    let (hits, _) = run(&store, &query("shared"));
    let times: Vec<u64> = hits.iter().map(|h| h.created_at).collect();
    assert_eq!(times, vec![300, 200, 100], "newest-first ordering");
}

#[test]
fn limit_and_early_stop() {
    let store = store_with_index();
    for i in 0..10u8 {
        insert(&store, make_event(i + 1, "common token", 100 + i as u64), 100_000 + i as u64);
    }
    let mut q = query("common");
    q.limit = 3;
    let (hits, status) = run(&store, &q);
    assert_eq!(hits.len(), 3, "limit caps the result count");
    assert_eq!(
        status,
        TextSearchStatus::Partial { budget_exhausted: false },
        "hitting the limit reports Partial(non-budget)"
    );

    // Visitor Break stops early and reports Complete for what was asked.
    let mut seen = 0usize;
    let status2 = store
        .text_search_visit(&query("common"), &mut |_hit| {
            seen += 1;
            if seen >= 2 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .unwrap();
    assert_eq!(seen, 2, "visitor Break stops the scan");
    assert_eq!(status2, TextSearchStatus::Complete);
}

#[test]
fn empty_query_is_complete_and_empty() {
    let store = store_with_index();
    insert(&store, make_event(0x01, "anything here", 100), 100_000);
    let (hits, status) = run(&store, &query("   ,.  "));
    assert!(hits.is_empty());
    assert_eq!(status, TextSearchStatus::Complete);
}

#[test]
fn delete_removes_hit() {
    let store = store_with_index();
    let ev = make_event(0x01, "deletable token", 100);
    let id = ev.id_bytes().unwrap();
    insert(&store, ev, 100_000);
    assert_eq!(run(&store, &query("deletable")).0.len(), 1);

    store.delete_by_filter(DeleteFilter::ByIds(vec![id])).unwrap();
    let (hits, status) = run(&store, &query("deletable"));
    assert!(hits.is_empty(), "deleted event must not survive in the FTS index");
    assert_eq!(status, TextSearchStatus::Complete);
}

#[test]
fn unknown_scope_is_unsupported() {
    let store = store_with_index();
    let mut q = query("anything");
    q.scope = SearchScopeId::from_label("never.registered");
    let (hits, status) = run(&store, &q);
    assert!(hits.is_empty());
    assert_eq!(status, TextSearchStatus::Unsupported);
}

#[test]
fn no_index_installed_is_unsupported() {
    // A store with no installed specs returns Unsupported (trait default path).
    let store = MemEventStore::new();
    let (hits, status) = run(&store, &query("anything"));
    assert!(hits.is_empty());
    assert_eq!(status, TextSearchStatus::Unsupported);
}

#[test]
fn budget_exhausted_reports_partial() {
    let store = store_with_index();
    for i in 0..20u8 {
        insert(&store, make_event(i + 1, "budgeted token", 100 + i as u64), 100_000 + i as u64);
    }
    let mut q = query("budgeted");
    q.budget = TextSearchBudget { max_docs_scanned: 5, max_matches: 1_000 };
    let (_hits, status) = run(&store, &q);
    assert_eq!(
        status,
        TextSearchStatus::Partial { budget_exhausted: true },
        "scanning past max_docs_scanned reports budget exhaustion"
    );
}
