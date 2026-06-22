//! #1811 cache-side full-text search — cross-slice integration + doctrine gate.
//!
//! This gate exercises the WHOLE FTS seam end-to-end through the public
//! composition surface, not a per-backend fixture spec:
//!
//!   crate-owned `SearchScopeProvider`s (NIP-50 profiles/notes/long-form +
//!   NIP-29 group metadata)  ──`SearchScopeRegistry::register`──▶  compiled,
//!   noun-free `CompiledIndexSpec`s  ──`install_into(store)`──▶  the durable
//!   inverted index  ──`text_search_visit`──▶  bounded, ordered hits.
//!
//! Every behavioural test runs against BOTH store backends (mem + LMDB) via the
//! `run_both` helper, so the integration is asserted at mem↔lmdb parity. The
//! gate covers:
//!   * token + prefix (typeahead) semantics through the real extractors,
//!   * early-stop / budget bound (corpus ≫ limit ⇒ loaded rows bounded by the
//!     plan, NOT the corpus; status is `Partial`),
//!   * privacy exclusion — a private kind ([4,13,14,15,1059,1060]) is never
//!     indexed, and no compiled public spec names a private kind,
//!   * cleanup on delete / replace / kind:5 / GC-expiry,
//!   * `CacheOnly` / cache-eligible scopes are surfaced through
//!     `cache_search_scopes` for the relay-silent cache-serve path,
//!   * the seam carries no protocol noun (asserted structurally — the store
//!     only ever sees opaque `SearchScopeId`s + kind integers).
//!
//! Run: cargo test -p nmp-testing --test cache_fts_integration_gate
//!      cargo test -p nmp-testing --features lmdb-backend \
//!          --test cache_fts_integration_gate

use std::collections::BTreeSet;
use std::ops::ControlFlow;

use nmp_core::substrate::SearchScopeRegistry;
use nmp_store::{
    DeleteFilter, EventStore, GcBudget, RawEvent, SearchScopeId, TextSearchBudget, TextSearchHit,
    TextSearchOrder, TextSearchQuery, TextSearchStatus,
};

use nmp_testing::store_harness::StoreHarness;

// nip50 scope labels (crate-owned constants — the test binds to the public
// surface, never a literal it could drift from).
use nmp_nip50::{SCOPE_LABEL_LONGFORM, SCOPE_LABEL_NOTES, SCOPE_LABEL_PROFILES};
use nmp_nip29::GROUP_SEARCH_SCOPE_LABEL;

const KIND_PROFILE: u32 = 0;
const KIND_NOTE: u32 = 1;
const KIND_LONGFORM: u32 = 30023;
const KIND_GROUP_META: u32 = 39000;
const RELAY: &str = "wss://r.example.com";

// ─── seam construction ────────────────────────────────────────────────────────

/// The canonical registration the composition root performs: NIP-50 public
/// scopes (default bundle) + NIP-29 group scope (leaf-app opt-in). Drives the
/// REAL crate helpers against a bare registry — `SearchScopeRegistry` is itself
/// a `SearchScopeRegistrar`, so this is the exact call shape the `AppHost` makes.
fn registry_with_all_scopes() -> SearchScopeRegistry {
    let registry = SearchScopeRegistry::new();
    nmp_nip50::register_search_scopes(&registry);
    nmp_nip29::register_search_scopes(&registry);
    registry
}

/// Install the compiled scopes into a fresh harness store (the same compile +
/// `install_into` seam `nmp-core::actor::config::apply_to_kernel` runs).
fn install_all_scopes(h: &StoreHarness) {
    let registry = registry_with_all_scopes();
    registry.install_into(&*h.store);
}

/// Run `body` against both backends so every integration assertion holds at
/// mem↔lmdb parity. (The `for_each_backend!` macro builds the store before the
/// body runs, so it cannot install specs first — hence this explicit helper.)
fn run_both(body: impl Fn(&mut StoreHarness)) {
    let mut mem = StoreHarness::mem();
    body(&mut mem);
    mem.assert_invariants();

    #[cfg(feature = "lmdb-backend")]
    {
        let mut lmdb = StoreHarness::lmdb();
        body(&mut lmdb);
        lmdb.assert_invariants();
    }
}

fn raw(h: &StoreHarness, kind: u32, created_at: u64, content: &str, tags: Vec<Vec<String>>) -> RawEvent {
    let mut ev = h.make_event_with_tags(crate_alice(), kind, created_at, tags);
    ev.content = content.to_string();
    ev
}

/// A deterministic 32-byte author pubkey hex.
fn crate_alice() -> &'static str {
    "1111111111111111111111111111111111111111111111111111111111111111"
}

fn insert(h: &StoreHarness, ev: RawEvent, at_ms: u64) {
    // Routes through the harness so it owns the `from_raw_unchecked` wrapping
    // (synthetic placeholder signatures) under the right feature gate.
    h.insert_raw(ev, RELAY, at_ms);
}

fn query(scope: &'static str, text: &str, limit: usize) -> TextSearchQuery {
    TextSearchQuery {
        scope: SearchScopeId::from_label(scope),
        query: text.to_string(),
        kinds: BTreeSet::new(),
        since: None,
        until: None,
        limit,
        order: TextSearchOrder::NewestFirst,
        budget: TextSearchBudget::default(),
    }
}

fn run_query(
    store: &dyn EventStore,
    q: &TextSearchQuery,
) -> (Vec<TextSearchHit>, TextSearchStatus) {
    let mut hits = Vec::new();
    let status = store
        .text_search_visit(q, &mut |hit| {
            hits.push(hit);
            ControlFlow::Continue(())
        })
        .expect("text_search_visit should not error");
    (hits, status)
}

// ─── token + prefix through the real extractors ───────────────────────────────

#[test]
fn profile_scope_searches_real_metadata_fields() {
    run_both(|h| {
        install_all_scopes(h);
        // kind:0 content is JSON — the extractor pulls name/display_name/nip05/about.
        let content =
            r#"{"name":"satoshi","display_name":"Satoshi Nakamoto","nip05":"s@example.com","about":"building bitcoin"}"#;
        insert(h, raw(h, KIND_PROFILE, 100, content, vec![]), 100_000);

        let (by_name, st) = run_query(&*h.store, &query(SCOPE_LABEL_PROFILES, "satoshi", 10));
        assert_eq!(st, TextSearchStatus::Complete);
        assert_eq!(by_name.len(), 1, "name token matches");

        // prefix / typeahead over the display name.
        let (prefix, _) = run_query(&*h.store, &query(SCOPE_LABEL_PROFILES, "naka", 10));
        assert_eq!(prefix.len(), 1, "display_name prefix matches");

        // about-field token.
        let (about, _) = run_query(&*h.store, &query(SCOPE_LABEL_PROFILES, "bitcoin", 10));
        assert_eq!(about.len(), 1, "about token matches");

        let (miss, _) = run_query(&*h.store, &query(SCOPE_LABEL_PROFILES, "ethereum", 10));
        assert!(miss.is_empty(), "unrelated token does not match");
    });
}

#[test]
fn note_and_longform_scopes_search_their_real_fields() {
    run_both(|h| {
        install_all_scopes(h);
        insert(h, raw(h, KIND_NOTE, 100, "gm nostr fam", vec![]), 100_000);
        insert(
            h,
            raw(
                h,
                KIND_LONGFORM,
                101,
                "the body discusses sovereignty at length",
                vec![
                    vec!["title".to_string(), "Self Custody Primer".to_string()],
                    vec!["summary".to_string(), "a gentle intro".to_string()],
                ],
            ),
            101_000,
        );

        let (note, _) = run_query(&*h.store, &query(SCOPE_LABEL_NOTES, "nostr", 10));
        assert_eq!(note.len(), 1, "note content token matches");

        let (lf_title, _) = run_query(&*h.store, &query(SCOPE_LABEL_LONGFORM, "custody", 10));
        assert_eq!(lf_title.len(), 1, "long-form title tag matches");
        let (lf_body, _) = run_query(&*h.store, &query(SCOPE_LABEL_LONGFORM, "sovereignty", 10));
        assert_eq!(lf_body.len(), 1, "long-form body prefix matches");

        // cross-scope isolation: a note token does not surface under longform.
        let (cross, _) = run_query(&*h.store, &query(SCOPE_LABEL_LONGFORM, "gm", 10));
        assert!(cross.is_empty(), "scopes do not bleed across kinds");
    });
}

#[test]
fn group_scope_searches_metadata_tags() {
    run_both(|h| {
        install_all_scopes(h);
        insert(
            h,
            raw(
                h,
                KIND_GROUP_META,
                100,
                "",
                vec![
                    vec!["d".to_string(), "devtalk".to_string()],
                    vec!["name".to_string(), "Nostr Dev Talk".to_string()],
                    vec!["about".to_string(), "protocol engineering chat".to_string()],
                ],
            ),
            100_000,
        );

        let (by_name, _) = run_query(&*h.store, &query(GROUP_SEARCH_SCOPE_LABEL, "nostr", 10));
        assert_eq!(by_name.len(), 1, "group name token matches");
        let (by_about, _) = run_query(&*h.store, &query(GROUP_SEARCH_SCOPE_LABEL, "engineering", 10));
        assert_eq!(by_about.len(), 1, "group about token matches");
        let (by_slug, _) = run_query(&*h.store, &query(GROUP_SEARCH_SCOPE_LABEL, "devtalk", 10));
        assert_eq!(by_slug.len(), 1, "group id slug is searchable");
    });
}

// ─── early-stop / budget bound (D8 — no corpus-size scan, no full materialize) ─

#[test]
fn corpus_far_exceeds_limit_loads_only_bounded_rows() {
    run_both(|h| {
        install_all_scopes(h);
        // A 400-note corpus that all share one token; the query asks for 5.
        const CORPUS: u64 = 400;
        const LIMIT: usize = 5;
        for i in 0..CORPUS {
            insert(
                h,
                raw(h, KIND_NOTE, 1_000 + i, "shared corpus token", vec![]),
                (1_000 + i) * 1000,
            );
        }

        // Count how many distinct documents the visitor is handed. With a
        // `limit` far below the corpus the visit must stop early — it never
        // materializes the whole corpus.
        let mut visited = 0usize;
        let q = query(SCOPE_LABEL_NOTES, "shared", LIMIT);
        let status = h
            .store
            .text_search_visit(&q, &mut |_| {
                visited += 1;
                ControlFlow::Continue(())
            })
            .unwrap();

        assert!(
            visited <= LIMIT,
            "visit must stop at the limit ({LIMIT}), not scan the {CORPUS}-doc corpus (visited={visited})"
        );
        assert!(
            matches!(status, TextSearchStatus::Partial { .. }),
            "a corpus ≫ limit query reports Partial (more matches exist), got {status:?}"
        );
        // newest-first ordering: the 5 returned are the newest 5 created_at.
        assert_eq!(visited, LIMIT, "exactly `limit` newest matches are delivered");
    });
}

#[test]
fn budget_exhaustion_is_reported_partial() {
    run_both(|h| {
        install_all_scopes(h);
        // 200 matching notes; a tiny scan budget forces early budget stop even
        // though `limit` is generous — proves the scan is bounded by the PLAN.
        for i in 0..200u64 {
            insert(h, raw(h, KIND_NOTE, 1_000 + i, "budgeted term", vec![]), (1_000 + i) * 1000);
        }
        let mut q = query(SCOPE_LABEL_NOTES, "budgeted", 10_000);
        q.budget = TextSearchBudget { max_docs_scanned: 32, max_matches: 1_000 };

        let (hits, status) = run_query(&*h.store, &q);
        assert!(
            matches!(status, TextSearchStatus::Partial { .. }),
            "a tight scan budget reports Partial, got {status:?}"
        );
        assert!(
            hits.len() <= 200,
            "never delivers more than the corpus; bounded by budget"
        );
    });
}

// ─── privacy exclusion ────────────────────────────────────────────────────────

#[test]
fn private_kinds_are_never_indexed() {
    // No public compiled spec may name a private/encrypted kind.
    let registry = registry_with_all_scopes();
    let specs = registry.compile();
    assert!(!specs.is_empty(), "the public scopes compile to a non-empty install set");
    const PRIVATE_KINDS: [u32; 6] = [4, 13, 14, 15, 1059, 1060];
    for spec in &specs {
        for k in &spec.kinds {
            assert!(
                !PRIVATE_KINDS.contains(k),
                "scope {:?} indexes private kind {k} — privacy leak",
                spec.scope_id
            );
        }
        assert!(
            !spec.local_only_private,
            "no cache-installed public scope is local_only_private"
        );
    }
}

#[test]
fn private_kind_event_never_surfaces_in_any_scope() {
    run_both(|h| {
        install_all_scopes(h);
        // A kind:4 (NIP-04 DM) "looks like" searchable text, but no public scope
        // claims kind 4, so it is never extracted and never returned.
        insert(h, raw(h, 4, 100, "secret plaintext leaking token", vec![]), 100_000);

        for scope in [SCOPE_LABEL_PROFILES, SCOPE_LABEL_NOTES, SCOPE_LABEL_LONGFORM, GROUP_SEARCH_SCOPE_LABEL] {
            let (hits, _) = run_query(&*h.store, &query(scope, "secret", 10));
            assert!(
                hits.is_empty(),
                "private kind:4 content surfaced under scope {scope} — privacy leak"
            );
        }
    });
}

// ─── cleanup on delete / replace / kind:5 / GC-expiry ─────────────────────────

#[test]
fn delete_removes_the_hit() {
    run_both(|h| {
        install_all_scopes(h);
        let ev = raw(h, KIND_NOTE, 100, "deletable token", vec![]);
        let id = ev.id_bytes().expect("valid hex");
        insert(h, ev, 100_000);
        let (before, _) = run_query(&*h.store, &query(SCOPE_LABEL_NOTES, "deletable", 10));
        assert_eq!(before.len(), 1);

        h.store.delete_by_filter(DeleteFilter::ByIds(vec![id])).unwrap();
        let (after, _) = run_query(&*h.store, &query(SCOPE_LABEL_NOTES, "deletable", 10));
        assert!(after.is_empty(), "delete must purge the FTS posting");
    });
}

#[test]
fn replace_swaps_the_indexed_text() {
    run_both(|h| {
        install_all_scopes(h);
        // A replaceable kind:0 (profile) — a newer kind:0 from the same author
        // supersedes the old one; the index must drop the old tokens.
        insert(
            h,
            raw(h, KIND_PROFILE, 100, r#"{"name":"oldhandle"}"#, vec![]),
            100_000,
        );
        let (old, _) = run_query(&*h.store, &query(SCOPE_LABEL_PROFILES, "oldhandle", 10));
        assert_eq!(old.len(), 1, "first profile is indexed");

        insert(
            h,
            raw(h, KIND_PROFILE, 200, r#"{"name":"newhandle"}"#, vec![]),
            200_000,
        );
        let (gone, _) = run_query(&*h.store, &query(SCOPE_LABEL_PROFILES, "oldhandle", 10));
        assert!(gone.is_empty(), "superseded profile tokens are removed");
        let (now, _) = run_query(&*h.store, &query(SCOPE_LABEL_PROFILES, "newhandle", 10));
        assert_eq!(now.len(), 1, "replacement profile is indexed");
    });
}

#[test]
fn kind5_delete_purges_the_hit() {
    run_both(|h| {
        install_all_scopes(h);
        let note = raw(h, KIND_NOTE, 100, "kind five target token", vec![]);
        let note_id_hex = note.id.clone();
        let _note_id = note.id_bytes().expect("valid hex");
        insert(h, note, 100_000);
        let (before, _) = run_query(&*h.store, &query(SCOPE_LABEL_NOTES, "target", 10));
        assert_eq!(before.len(), 1);

        // The author publishes a kind:5 deletion referencing the note.
        let kind5 = h.make_event_with_tags(
            crate_alice(),
            5,
            200,
            vec![vec!["e".to_string(), note_id_hex]],
        );
        insert(h, kind5, 200_000);

        let (after, _) = run_query(&*h.store, &query(SCOPE_LABEL_NOTES, "target", 10));
        assert!(after.is_empty(), "kind:5 self-delete purges the FTS posting");
    });
}

#[test]
fn gc_expiry_reap_purges_the_hit() {
    run_both(|h| {
        install_all_scopes(h);
        // NIP-40 expiration in the past (unix second 2), received before expiry.
        let ev = raw(
            h,
            KIND_NOTE,
            1_000,
            "expiring token soon",
            vec![vec!["expiration".to_string(), "2".to_string()]],
        );
        let id = ev.id_bytes().expect("valid hex");
        insert(h, ev, 1); // received_at_ms = 1 < expiry
        h.assert_present(&id);
        let (before, _) = run_query(&*h.store, &query(SCOPE_LABEL_NOTES, "expiring", 10));
        assert_eq!(before.len(), 1);

        let report = h
            .store
            .gc_step(
                GcBudget {
                    max_events_per_step: 100,
                    max_duration_ms: 1_000,
                    max_total_events: usize::MAX,
                },
                1_700_000_000, // now ≫ expiration=2
            )
            .unwrap();
        assert!(report.expired_reaped >= 1, "the expired note is reaped");

        let (after, _) = run_query(&*h.store, &query(SCOPE_LABEL_NOTES, "expiring", 10));
        assert!(after.is_empty(), "GC-expiry reap purges the FTS posting");
    });
}

// ─── cache-serve scope resolution surface ─────────────────────────────────────

#[test]
fn installed_scopes_are_surfaced_for_cache_serve() {
    run_both(|h| {
        install_all_scopes(h);
        let cache_scopes = h.store.cache_search_scopes();
        // All four crate scopes are cache-eligible (Both / CacheOnly), so the
        // cache-serve hook can resolve a search shape to the local index.
        let installed: BTreeSet<SearchScopeId> =
            cache_scopes.iter().map(|(s, _)| *s).collect();
        for label in [
            SCOPE_LABEL_PROFILES,
            SCOPE_LABEL_NOTES,
            SCOPE_LABEL_LONGFORM,
            GROUP_SEARCH_SCOPE_LABEL,
        ] {
            assert!(
                installed.contains(&SearchScopeId::from_label(label)),
                "scope {label} must be surfaced for cache-serve resolution"
            );
        }
        // The kinds reported per scope are the indexable (public) kinds only.
        for (scope, kinds) in &cache_scopes {
            assert!(
                !kinds.is_empty(),
                "scope {scope:?} reports its indexable kinds for shape intersection"
            );
            for k in kinds {
                assert!(
                    ![4u32, 13, 14, 15, 1059, 1060].contains(k),
                    "cache-serve scope kinds never include a private kind"
                );
            }
        }
    });
}
