//! #1811 — durable LMDB full-text-search index tests.
//!
//! Holds the LMDB backend to the SAME FTS contract as the mem backend
//! (`mem/tests/fts_tests.rs`), via a shared `run_parity` body parameterized over
//! the store + event builder so the two backends are exercised by one test body
//! (token match, prefix typeahead, multi-token AND, newest-first, limit/early-
//! stop, since/until, delete/replace/kind5/GC cleanup). Plus LMDB-specifics:
//!
//!   * `early_stop_bound` — corpus ≫ limit loads rows bounded by the PLAN
//!     (budget), not the corpus.
//!   * `index_survives_reopen` — the index is durable (sub-dbs), not in-memory.
//!   * `additional_dbs_slot_present` — the three FTS sub-dbs occupy real slots
//!     (open succeeds with `NMP_ADDITIONAL_DBS` covering those slots).
//!   * `tokenizer_version_rebuild` — a fresh open backfills the gate key.

#![cfg(all(test, feature = "lmdb-backend"))]

use std::collections::BTreeSet;
use std::ops::ControlFlow;
use std::sync::Arc;

use crate::text_search::{
    CompiledIndexSpec, ExtractFn, SearchField, SearchScopeId, TextSearchBudget, TextSearchHit,
    TextSearchOrder, TextSearchQuery, TextSearchStatus,
};
use crate::types::{RawEvent, StoredEvent};
use crate::{EventStore, LmdbEventStore};

use super::test_fixtures::{open_tmp, signed_event, signed_event_with_keys, verified};

// #1882 — cache-FTS budget/bound/token-length defect regression tests (split out
// to keep this file under the 500-LOC ceiling). Reuses the helpers below.
mod contract_tests;
mod defects_1882;

const TEST_KIND: u32 = 1;
const TEST_LABEL: &str = "test.note";
const RELAY: &str = "wss://r.example.com";

fn scope_id() -> SearchScopeId {
    SearchScopeId::from_label(TEST_LABEL)
}

fn fixture_spec() -> CompiledIndexSpec {
    let extract: Arc<ExtractFn> =
        Arc::new(|ev: &StoredEvent| vec![(SearchField::new(0), ev.raw.content.clone())]);
    CompiledIndexSpec {
        scope_id: scope_id(),
        kinds: BTreeSet::from([TEST_KIND]),
        extract,
        local_only_private: false,
    }
}

fn store_with_index() -> (LmdbEventStore, tempfile::TempDir) {
    let (store, dir) = open_tmp();
    store.install_search_index_specs(vec![fixture_spec()]);
    (store, dir)
}

fn ins(store: &LmdbEventStore, ev: RawEvent, at_ms: u64) {
    store
        .insert(verified(ev), &RELAY.to_string(), at_ms)
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

fn run(store: &LmdbEventStore, q: &TextSearchQuery) -> (Vec<TextSearchHit>, TextSearchStatus) {
    let mut hits = Vec::new();
    let status = store
        .text_search_visit(q, &mut |hit| {
            hits.push(hit);
            ControlFlow::Continue(())
        })
        .unwrap();
    (hits, status)
}

// ─── LMDB-specifics ──────────────────────────────────────────────────────────

#[test]
fn budget_exhausted_reports_partial() {
    let (store, _d) = store_with_index();
    for i in 0..20u64 {
        ins(
            &store,
            signed_event(TEST_KIND, 100 + i, "budgeted token", None),
            100_000 + i,
        );
    }
    let mut q = query("budgeted");
    q.budget = TextSearchBudget {
        max_docs_scanned: 5,
        max_matches: 1_000,
    };
    let (_hits, status) = run(&store, &q);
    assert_eq!(
        status,
        TextSearchStatus::Partial {
            budget_exhausted: true
        },
        "scanning past max_docs_scanned reports budget exhaustion"
    );
}

#[test]
fn early_stop_bound() {
    // Corpus ≫ limit: the planner must early-stop. With a small max_docs_scanned
    // budget the scan reports Partial(budget) rather than walking the corpus.
    let (store, _d) = store_with_index();
    for i in 0..200u64 {
        ins(
            &store,
            signed_event(TEST_KIND, 1_000 + i, "corpus token", None),
            1_000_000 + i,
        );
    }
    let mut q = query("corpus");
    q.limit = 5;
    q.budget = TextSearchBudget {
        max_docs_scanned: 10,
        max_matches: 1_000,
    };
    let (hits, status) = run(&store, &q);
    assert!(hits.len() <= 5, "result count bounded by limit");
    assert_eq!(
        status,
        TextSearchStatus::Partial {
            budget_exhausted: true
        },
        "loaded rows bounded by the plan budget, not the 200-doc corpus"
    );
}

#[test]
fn index_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let id;
    {
        let store = LmdbEventStore::open(dir.path()).unwrap();
        store.install_search_index_specs(vec![fixture_spec()]);
        let ev = signed_event(TEST_KIND, 100, "persistent token", None);
        id = ev.id_bytes().unwrap();
        store
            .insert(verified(ev), &RELAY.to_string(), 100_000)
            .unwrap();
    }
    // Reopen: the postings sub-db is durable. Re-install the spec set (extractors
    // are not persisted — only the inverted index is) so queries resolve scope.
    let store = LmdbEventStore::open(dir.path()).unwrap();
    store.install_search_index_specs(vec![fixture_spec()]);
    let (hits, status) = run(&store, &query("persistent"));
    assert_eq!(status, TextSearchStatus::Complete);
    assert_eq!(hits.len(), 1, "FTS index persists across reopen");
    assert_eq!(hits[0].event_id, Some(id));
}

#[test]
fn additional_dbs_slot_present() {
    // The three FTS sub-dbs occupy real slots: open succeeds and a search
    // round-trips end-to-end.
    let (store, _d) = store_with_index();
    ins(
        &store,
        signed_event(TEST_KIND, 100, "slot token", None),
        100_000,
    );
    assert_eq!(
        run(&store, &query("slot")).0.len(),
        1,
        "all FTS sub-dbs are open"
    );
}

#[test]
fn backfill_indexes_preexisting_events() {
    // Events stored BEFORE specs are installed must be backfilled on install.
    let (store, _d) = open_tmp();
    ins(
        &store,
        signed_event(TEST_KIND, 100, "backfilled token", None),
        100_000,
    );
    // Not yet installed → Unsupported.
    let (_h, status) = run(&store, &query("backfilled"));
    assert_eq!(status, TextSearchStatus::Unsupported);
    // Installing runs the one-time backfill over indexable kinds.
    store.install_search_index_specs(vec![fixture_spec()]);
    let (hits, _) = run(&store, &query("backfilled"));
    assert_eq!(
        hits.len(),
        1,
        "pre-existing event is backfilled into the index"
    );
}
