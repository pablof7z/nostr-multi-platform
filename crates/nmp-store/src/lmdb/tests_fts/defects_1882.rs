//! #1882 — codex-review cache-FTS defect regressions (LMDB backend), kept in
//! mem↔lmdb parity with `mem/tests/fts_tests.rs`:
//!   * #2  — a budget-exhausted prefix scan still passes the exact-token AND
//!           filter (never a false positive / superset).
//!   * #3  — the exact-term AND is bounded by the collected candidate set, not by
//!           materializing a common term's posting list (D8).
//!   * #6  — an over-cap token is truncated identically at index and remove time,
//!           so delete/replace leave no stale postings (u16 codec overflow).

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::text_search::{
    CompiledIndexSpec, ExtractFn, SearchField, SearchScopeId, TextSearchBudget, TextSearchStatus,
};
use crate::types::{DeleteFilter, StoredEvent};
use crate::EventStore;

use super::*;

#[test]
fn budget_exhaustion_never_yields_and_false_positive() {
    // The newest "common" docs lack "alpha"; older ones have both. A budget that
    // stops the prefix scan inside the non-alpha run must NOT emit those docs.
    let (store, _d) = store_with_index();
    for at in [200u64, 201, 202, 203] {
        ins(
            &store,
            signed_event(TEST_KIND, at, "common", None),
            at * 1_000,
        );
    }
    for at in [100u64, 101, 102, 103] {
        ins(
            &store,
            signed_event(TEST_KIND, at, "common alpha", None),
            at * 1_000,
        );
    }
    let mut q = query("alpha common"); // exact=["alpha"], prefix="common"
    q.budget = TextSearchBudget {
        max_docs_scanned: 6,
        max_matches: 1_000,
    };
    let (hits, status) = run(&store, &q);
    assert_eq!(
        status,
        TextSearchStatus::Partial {
            budget_exhausted: true
        },
        "prefix scan exhausted the budget"
    );
    let times: Vec<u64> = hits.iter().map(|h| h.created_at).collect();
    assert_eq!(
        times,
        vec![103, 102],
        "only docs that actually contain 'alpha' survive"
    );
    assert!(
        hits.iter().all(|h| h.created_at < 200),
        "no doc missing the required exact token is ever returned"
    );
}

#[test]
fn common_exact_term_bounded_by_candidate_set() {
    // The exact-term AND is a point look-up per collected candidate, not a scan of
    // the exact term's posting list. A common exact term + a RARE prefix under a
    // tight budget completes correctly without the common term touching the budget.
    let (store, _d) = store_with_index();
    for at in 100u64..120 {
        ins(
            &store,
            signed_event(TEST_KIND, at, "common", None),
            at * 1_000,
        );
    }
    for at in [200u64, 201, 202] {
        ins(
            &store,
            signed_event(TEST_KIND, at, "common rareword", None),
            at * 1_000,
        );
    }
    let mut q = query("common rare"); // exact=["common"], prefix="rare"
    q.budget = TextSearchBudget {
        max_docs_scanned: 5,
        max_matches: 1_000,
    };
    let (hits, status) = run(&store, &q);
    assert_eq!(
        status,
        TextSearchStatus::Complete,
        "only the 3 rare-prefix postings are scanned"
    );
    let times: Vec<u64> = hits.iter().map(|h| h.created_at).collect();
    assert_eq!(
        times,
        vec![202, 201, 200],
        "AND with the common term keeps exactly the 3 candidates"
    );
}

#[test]
fn overlong_token_delete_no_stale_postings() {
    // An over-cap token is truncated to MAX_TOKEN_BYTES at index time, so the
    // doc-term codec's u16 length never overflows and remove-time reconstructs the
    // SAME token → delete leaves no stale posting (the durable-codec bug).
    let (store, _d) = store_with_index();
    let long = "z".repeat(70_000); // > u16::MAX bytes uncapped → capped to 128
    let ev = signed_event(TEST_KIND, 100, &long, None);
    let id = ev.id_bytes().unwrap();
    ins(&store, ev, 100_000);
    assert_eq!(
        run(&store, &query("zzzz")).0.len(),
        1,
        "capped long token is searchable by prefix"
    );

    store
        .delete_by_filter(DeleteFilter::ByIds(vec![id]))
        .unwrap();
    let (hits, _) = run(&store, &query("zzzz"));
    assert!(
        hits.is_empty(),
        "no stale posting survives delete of an over-cap token"
    );
}

#[test]
fn overlong_token_replace_drops_old() {
    // Replace must drop the superseded doc's (capped) over-long token and index the
    // new one — proving index/remove agree on the truncated token.
    let scope = SearchScopeId::from_label("test.replaceable.long");
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
    let old_content = format!("{} alpha", "z".repeat(70_000));
    let new_content = format!("{} alpha", "y".repeat(70_000));
    let old = signed_event_with_keys(&keys, 10000, 100, &old_content, None);
    let new = signed_event_with_keys(&keys, 10000, 200, &new_content, None);
    store
        .insert(verified(old), &RELAY.to_string(), 100_000)
        .unwrap();
    store
        .insert(verified(new), &RELAY.to_string(), 200_000)
        .unwrap();

    let mut q_old = query("zzzz");
    q_old.scope = scope;
    assert!(
        run(&store, &q_old).0.is_empty(),
        "superseded over-long token must be gone"
    );
    let mut q_new = query("yyyy");
    q_new.scope = scope;
    assert_eq!(
        run(&store, &q_new).0.len(),
        1,
        "new over-long token is indexed"
    );
}
