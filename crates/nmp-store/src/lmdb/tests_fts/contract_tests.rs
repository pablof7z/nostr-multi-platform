//! Mem-parity FTS contract tests for the LMDB backend.

use std::collections::BTreeSet;
use std::ops::ControlFlow;
use std::sync::Arc;

use crate::text_search::{CompiledIndexSpec, ExtractFn, SearchField, SearchScopeId};
use crate::types::{DeleteFilter, RawEvent, StoredEvent};
use crate::EventStore;

use super::*;

#[test]
fn token_match() {
    let (store, _d) = store_with_index();
    ins(
        &store,
        signed_event(TEST_KIND, 100, "hello satoshi nakamoto", None),
        100_000,
    );
    let (hits, status) = run(&store, &query("nakamoto"));
    assert_eq!(status, TextSearchStatus::Complete);
    assert_eq!(hits.len(), 1);
}

#[test]
fn prefix_match() {
    let (store, _d) = store_with_index();
    ins(
        &store,
        signed_event(TEST_KIND, 100, "hello satoshi", None),
        100_000,
    );
    let (hits, status) = run(&store, &query("sato"));
    assert_eq!(status, TextSearchStatus::Complete);
    assert_eq!(hits.len(), 1, "typeahead prefix matches the token");
    let (none, _) = run(&store, &query("xyz"));
    assert!(none.is_empty());
}

#[test]
fn multi_token_and() {
    let (store, _d) = store_with_index();
    ins(
        &store,
        signed_event(TEST_KIND, 100, "alpha beta gamma", None),
        100_000,
    );
    ins(
        &store,
        signed_event(TEST_KIND, 101, "alpha delta", None),
        101_000,
    );
    let (hits, status) = run(&store, &query("alpha bet"));
    assert_eq!(status, TextSearchStatus::Complete);
    assert_eq!(hits.len(), 1, "both 'alpha' AND 'bet*' must be present");
    assert_eq!(hits[0].created_at, 100);
}

#[test]
fn newest_first() {
    let (store, _d) = store_with_index();
    ins(
        &store,
        signed_event(TEST_KIND, 100, "shared term", None),
        100_000,
    );
    ins(
        &store,
        signed_event(TEST_KIND, 300, "shared term", None),
        300_000,
    );
    ins(
        &store,
        signed_event(TEST_KIND, 200, "shared term", None),
        200_000,
    );
    let (hits, _) = run(&store, &query("shared"));
    let times: Vec<u64> = hits.iter().map(|h| h.created_at).collect();
    assert_eq!(times, vec![300, 200, 100], "newest-first ordering");
}

#[test]
fn limit_reports_partial() {
    let (store, _d) = store_with_index();
    for i in 0..10u64 {
        ins(
            &store,
            signed_event(TEST_KIND, 100 + i, "common token", None),
            100_000 + i,
        );
    }
    let mut q = query("common");
    q.limit = 3;
    let (hits, status) = run(&store, &q);
    assert_eq!(hits.len(), 3, "limit caps the result count");
    assert_eq!(
        status,
        TextSearchStatus::Partial {
            budget_exhausted: false
        }
    );
}

#[test]
fn visitor_break_is_complete() {
    let (store, _d) = store_with_index();
    for i in 0..5u64 {
        ins(
            &store,
            signed_event(TEST_KIND, 100 + i, "common token", None),
            100_000 + i,
        );
    }
    let mut seen = 0usize;
    let status = store
        .text_search_visit(&query("common"), &mut |_h| {
            seen += 1;
            if seen >= 2 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .unwrap();
    assert_eq!(seen, 2);
    assert_eq!(status, TextSearchStatus::Complete);
}

#[test]
fn empty_query_is_complete_and_empty() {
    let (store, _d) = store_with_index();
    ins(
        &store,
        signed_event(TEST_KIND, 100, "anything here", None),
        100_000,
    );
    let (hits, status) = run(&store, &query("   ,.  "));
    assert!(hits.is_empty());
    assert_eq!(status, TextSearchStatus::Complete);
}

#[test]
fn since_until_filters() {
    let (store, _d) = store_with_index();
    ins(
        &store,
        signed_event(TEST_KIND, 100, "windowed token", None),
        100_000,
    );
    ins(
        &store,
        signed_event(TEST_KIND, 200, "windowed token", None),
        200_000,
    );
    ins(
        &store,
        signed_event(TEST_KIND, 300, "windowed token", None),
        300_000,
    );
    let mut q = query("windowed");
    q.since = Some(150);
    q.until = Some(250);
    let (hits, _) = run(&store, &q);
    let times: Vec<u64> = hits.iter().map(|h| h.created_at).collect();
    assert_eq!(
        times,
        vec![200],
        "only the in-window event survives since/until"
    );
}

#[test]
fn delete_removes_hit() {
    let (store, _d) = store_with_index();
    let ev = signed_event(TEST_KIND, 100, "deletable token", None);
    let id = ev.id_bytes().unwrap();
    ins(&store, ev, 100_000);
    assert_eq!(run(&store, &query("deletable")).0.len(), 1);

    store
        .delete_by_filter(DeleteFilter::ByIds(vec![id]))
        .unwrap();
    let (hits, status) = run(&store, &query("deletable"));
    assert!(
        hits.is_empty(),
        "deleted event must not survive in the FTS index"
    );
    assert_eq!(status, TextSearchStatus::Complete);
}

#[test]
fn replace_drops_old_indexes_new() {
    // A replaceable kind (10000) supersession must drop the old doc's tokens and
    // index the new one. Use a kind-10000 scope.
    let scope = SearchScopeId::from_label("test.replaceable");
    let extract: Arc<ExtractFn> =
        Arc::new(|ev: &StoredEvent| vec![(SearchField::new(0), ev.raw.content.clone())]);
    let spec = CompiledIndexSpec {
        scope_id: scope,
        kinds: BTreeSet::from([10000u32]),
        extract,
        local_only_private: false,
    };
    let (store, _d) = open_tmp();
    store.install_search_index_specs(vec![spec]);

    let keys = nostr::Keys::generate();
    let old = signed_event_with_keys(&keys, 10000, 100, "oldtoken alpha", None);
    let new = signed_event_with_keys(&keys, 10000, 200, "newtoken alpha", None);
    store
        .insert(verified(old), &RELAY.to_string(), 100_000)
        .unwrap();
    store
        .insert(verified(new), &RELAY.to_string(), 200_000)
        .unwrap();

    let mut q = query("oldtoken");
    q.scope = scope;
    assert!(
        run(&store, &q).0.is_empty(),
        "superseded token must be gone"
    );
    let mut q2 = query("newtoken");
    q2.scope = scope;
    assert_eq!(run(&store, &q2).0.len(), 1, "new event is indexed");
}

#[test]
fn kind5_delete_removes_hit() {
    let (store, _d) = store_with_index();
    let keys = nostr::Keys::generate();
    let target = signed_event_with_keys(&keys, TEST_KIND, 100, "kindfive token", None);
    let target_id = target.id_bytes().unwrap();
    store
        .insert(verified(target), &RELAY.to_string(), 100_000)
        .unwrap();
    assert_eq!(run(&store, &query("kindfive")).0.len(), 1);

    // Author-signed kind:5 deleting the target by e-tag.
    use nostr::prelude::*;
    let del = EventBuilder::new(Kind::from(5u16), "")
        .tag(Tag::event(nostr::EventId::from_slice(&target_id).unwrap()))
        .custom_created_at(Timestamp::from_secs(200))
        .sign_with_keys(&keys)
        .unwrap();
    let del_json = del.try_as_json().unwrap();
    let del_raw: RawEvent = serde_json::from_str(&del_json).unwrap();
    store
        .insert(verified(del_raw), &RELAY.to_string(), 200_000)
        .unwrap();

    let (hits, _) = run(&store, &query("kindfive"));
    assert!(
        hits.is_empty(),
        "kind:5-deleted target must not survive in FTS"
    );
}

#[test]
fn gc_expiry_reap_removes_hit() {
    let (store, _d) = store_with_index();
    use nostr::prelude::*;
    let keys = nostr::Keys::generate();
    let ev = EventBuilder::new(Kind::from(TEST_KIND as u16), "expiring token")
        .tag(Tag::expiration(Timestamp::from_secs(150)))
        .custom_created_at(Timestamp::from_secs(100))
        .sign_with_keys(&keys)
        .unwrap();
    let json = ev.try_as_json().unwrap();
    let raw: RawEvent = serde_json::from_str(&json).unwrap();
    store
        .insert(verified(raw), &RELAY.to_string(), 100_000)
        .unwrap();
    assert_eq!(run(&store, &query("expiring")).0.len(), 1);

    // GC past the expiry reaps the event AND its FTS rows.
    let budget = crate::types::GcBudget {
        max_events_per_step: 100,
        max_total_events: usize::MAX,
        max_duration_ms: 1_000,
    };
    store
        .gc_step_with_pins(budget, 1_000, &std::collections::HashSet::new())
        .unwrap();
    let (hits, _) = run(&store, &query("expiring"));
    assert!(
        hits.is_empty(),
        "expiry-reaped event must not survive in FTS"
    );
}

#[test]
fn unknown_scope_is_unsupported() {
    let (store, _d) = store_with_index();
    let mut q = query("anything");
    q.scope = SearchScopeId::from_label("never.registered");
    let (hits, status) = run(&store, &q);
    assert!(hits.is_empty());
    assert_eq!(status, TextSearchStatus::Unsupported);
}

#[test]
fn no_index_installed_is_unsupported() {
    let (store, _d) = open_tmp();
    let (hits, status) = run(&store, &query("anything"));
    assert!(hits.is_empty());
    assert_eq!(status, TextSearchStatus::Unsupported);
}

#[test]
fn kinds_narrowing_uses_primary_kind() {
    // Scope spans two kinds; a kinds-narrowed query returns only the asked kind.
    let scope = SearchScopeId::from_label("test.multikind");
    let extract: Arc<ExtractFn> =
        Arc::new(|ev: &StoredEvent| vec![(SearchField::new(0), ev.raw.content.clone())]);
    let spec = CompiledIndexSpec {
        scope_id: scope,
        kinds: BTreeSet::from([1u32, 30023u32]),
        extract,
        local_only_private: false,
    };
    let (store, _d) = open_tmp();
    store.install_search_index_specs(vec![spec]);
    store
        .insert(
            verified(signed_event(1, 100, "narrow token", None)),
            &RELAY.to_string(),
            100_000,
        )
        .unwrap();
    store
        .insert(
            verified(signed_event(30023, 200, "narrow token", Some("d1"))),
            &RELAY.to_string(),
            200_000,
        )
        .unwrap();

    let mut q = query("narrow");
    q.scope = scope;
    assert_eq!(
        run(&store, &q).0.len(),
        2,
        "both kinds match with no narrowing"
    );
    q.kinds = BTreeSet::from([30023u32]);
    let (hits, _) = run(&store, &q);
    assert_eq!(hits.len(), 1, "kinds narrowing keeps only kind 30023");
    assert_eq!(hits[0].created_at, 200);
}
