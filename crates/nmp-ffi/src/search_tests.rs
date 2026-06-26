//! Tests for the higher-order NIP-50 search FFI surface.

use super::*;
use crate::{nmp_app_free, nmp_app_new};
use nmp_core::substrate::SearchScopeRegistry;
use nmp_nip50::{
    decode_search_results_snapshot, install_search_relay_source, SearchHitSource,
    SearchRelaySource, SearchScope, SearchTargets,
};
use nmp_store::{EventStore, MemEventStore, RawEvent, VerifiedEvent};
use std::collections::BTreeSet;
use std::ffi::CString;

/// A registered `SearchRelaySource` lets `open_search` resolve `UserPreferred`
/// to the kind:10007 set and `AppDefault` to the app default.
struct StubSource {
    preferred: Vec<String>,
    default: Vec<String>,
}
impl SearchRelaySource for StubSource {
    fn user_preferred(&self) -> Vec<String> {
        self.preferred.clone()
    }
    fn app_default(&self) -> Vec<String> {
        self.default.clone()
    }
}

#[test]
fn parse_search_request_runs_nip50_validation() {
    // Valid.
    let req = parse_search_request(
        r#"{"query":"nostr","scope":"Users","targets":"UserPreferred","max_hits":10}"#,
    )
    .expect("valid request");
    assert_eq!(req.query, "nostr");
    assert_eq!(req.max_hits, 10);

    // Whitespace-only query fails the bounded-query validation.
    assert!(
        parse_search_request(r#"{"query":"   ","scope":"Users","targets":"AppDefault"}"#).is_none()
    );

    // Malformed JSON.
    assert!(parse_search_request("not json").is_none());

    // Explicit targets + Kinds scope parse.
    let req = parse_search_request(
        r#"{"query":"a","scope":{"Kinds":[1,30023]},"targets":{"Explicit":["wss://r/"]}}"#,
    )
    .expect("kinds+explicit");
    assert!(matches!(req.scope, SearchScope::Kinds(_)));
    assert!(matches!(req.targets, SearchTargets::Explicit(_)));
}

#[test]
fn open_search_registers_typed_sidecar_under_session_key() {
    let app = nmp_app_new();
    // SAFETY: `nmp_app_new` never returns null.
    let app_ref = unsafe { &*app };
    install_search_relay_source(
        app_ref,
        Arc::new(StubSource {
            preferred: vec!["wss://search.nos.lol/".to_string()],
            default: Vec::new(),
        }),
    );

    let request = SearchRequest::new(
        "nostr",
        SearchScope::Users,
        SearchTargets::UserPreferred,
        Some(20),
    )
    .expect("request");
    let key = app_ref.open_search(request, "s1");
    assert_eq!(key, "nmp.nip50.search.s1");

    // The typed projection key is registered.
    assert!(app_ref
        .registered_typed_projection_keys()
        .contains(&"nmp.nip50.search.s1".to_string()));

    // The N50S sidecar is readable + decodable (empty hits before any event).
    let bytes = app_ref.search_snapshot_bytes("s1").expect("sidecar bytes");
    let snap = decode_search_results_snapshot(&bytes).expect("decode N50S");
    assert!(snap.hits.is_empty());

    nmp_app_free(app);
}

#[test]
fn close_search_tears_down_the_projection() {
    let app = nmp_app_new();
    // SAFETY: `nmp_app_new` never returns null.
    let app_ref = unsafe { &*app };
    install_search_relay_source(
        app_ref,
        Arc::new(StubSource {
            preferred: vec!["wss://search.nos.lol/".to_string()],
            default: Vec::new(),
        }),
    );

    let request = SearchRequest::new(
        "nostr",
        SearchScope::Users,
        SearchTargets::UserPreferred,
        None,
    )
    .expect("request");
    app_ref.open_search(request, "s2");
    assert!(app_ref
        .registered_typed_projection_keys()
        .contains(&"nmp.nip50.search.s2".to_string()));

    app_ref.close_search("s2");
    assert!(!app_ref
        .registered_typed_projection_keys()
        .contains(&"nmp.nip50.search.s2".to_string()));
    assert!(app_ref.search_snapshot_bytes("s2").is_none());

    // Idempotent: closing again is a no-op.
    app_ref.close_search("s2");

    nmp_app_free(app);
}

/// End-to-end transparency proof: with the default `PreferredRelaySource`
/// installed (exactly what `register_defaults` does via
/// `nmp_nip50::install_search_relay_source`), `open_search(UserPreferred)` fans
/// out to the source's PRIMARY relays (the user's published kind:10007 list),
/// and falls back to the app default when the user list is empty — with ZERO
/// per-relay app wiring at the call site.
#[test]
fn open_search_user_preferred_fans_out_to_installed_primary_relays() {
    let app = nmp_app_new();
    // SAFETY: `nmp_app_new` never returns null.
    let app_ref = unsafe { &*app };

    // Non-empty primary (the user's kind:10007 list) → UserPreferred uses it.
    install_search_relay_source(
        app_ref,
        Arc::new(StubSource {
            preferred: vec!["wss://user-search.example/".to_string()],
            default: vec!["wss://app-default.example/".to_string()],
        }),
    );
    let request = SearchRequest::new(
        "nostr",
        SearchScope::Users,
        SearchTargets::UserPreferred,
        Some(10),
    )
    .expect("request");
    app_ref.open_search(request, "user");
    assert_eq!(
        app_ref.search_session_relays("user"),
        vec!["wss://user-search.example/".to_string()],
        "UserPreferred must fan the search REQ out to the installed primary (kind:10007) relays"
    );

    // Empty primary → UserPreferred falls back to the app default.
    install_search_relay_source(
        app_ref,
        Arc::new(StubSource {
            preferred: Vec::new(),
            default: vec!["wss://app-default.example/".to_string()],
        }),
    );
    let req2 = SearchRequest::new(
        "nostr",
        SearchScope::Users,
        SearchTargets::UserPreferred,
        Some(10),
    )
    .expect("request");
    app_ref.open_search(req2, "fallback");
    assert_eq!(
        app_ref.search_session_relays("fallback"),
        vec!["wss://app-default.example/".to_string()],
        "UserPreferred with an empty user list must fall back to the app default"
    );

    nmp_app_free(app);
}

/// Behavior-preservation invariant (#2089) — a search targeting MANY relays
/// registers exactly ONE observed-projection sink, shared across every
/// relay-pinned interest. This keeps live relay-hit processing once per session
/// (not once per relay). Closing the session removes that single sink.
#[test]
fn multi_relay_search_shares_one_kernel_observer() {
    let app = nmp_app_new();
    // SAFETY: `nmp_app_new` never returns null.
    let app_ref = unsafe { &*app };
    let observers = app_ref.event_observers_handle();
    let before = nmp_core::__ffi_internal::rust_observer_count(&observers);

    // Three explicit relays → three pinned interests in one session.
    let request = SearchRequest::new(
        "nostr",
        SearchScope::Users,
        SearchTargets::Explicit(vec![
            "wss://r1.example/".to_string(),
            "wss://r2.example/".to_string(),
            "wss://r3.example/".to_string(),
        ]),
        Some(10),
    )
    .expect("request");
    app_ref.open_search(request, "multi");

    assert_eq!(
        app_ref.search_session_relays("multi").len(),
        3,
        "the session fans out to all three explicit relays"
    );
    assert_eq!(
        nmp_core::__ffi_internal::rust_observer_count(&observers),
        before + 1,
        "three relay-pinned interests share ONE kernel observer (not one per relay)"
    );

    app_ref.close_search("multi");
    assert_eq!(
        nmp_core::__ffi_internal::rust_observer_count(&observers),
        before,
        "closing the session removes its single shared observer"
    );

    nmp_app_free(app);
}

#[test]
fn open_search_without_source_is_cache_only_not_a_crash() {
    let app = nmp_app_new();
    // SAFETY: `nmp_app_new` never returns null.
    let app_ref = unsafe { &*app };
    // No source registered → UserPreferred resolves to empty (no relay fan-out)
    // but the projection + sidecar still register (cache-only search).
    let request = SearchRequest::new(
        "nostr",
        SearchScope::Users,
        SearchTargets::UserPreferred,
        None,
    )
    .expect("request");
    let key = app_ref.open_search(request, "s3");
    assert_eq!(key, "nmp.nip50.search.s3");
    assert!(app_ref.search_snapshot_bytes("s3").is_some());
    nmp_app_free(app);
}

// ===========================================================================
// #1882 — cache hits are search-text-filtered + `Cache`-provenanced
// ===========================================================================

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

/// Publish a `MemEventStore` into the app's kernel store slot, with the NIP-50
/// FTS scopes installed through the real `register_search_scopes` path (exactly
/// what `register_defaults` wires) and `events` inserted + indexed. This is the
/// store `open_search`'s cache scan reads via `text_search_visit`.
fn publish_store_with_notes(app: &NmpApp, events: &[(u8, &str, u64)]) {
    let registry = SearchScopeRegistry::new();
    nmp_nip50::register_search_scopes(&registry);
    let store = MemEventStore::new();
    registry.install_into(&store);
    for (id_byte, content, created_at) in events {
        store
            .insert(
                VerifiedEvent::from_raw_unchecked(raw_note(*id_byte, content, *created_at)),
                &"wss://cache/".to_string(),
                *created_at,
            )
            .expect("insert indexed note");
    }
    *app.event_store_handle().lock().expect("store slot") = Some(Arc::new(store));
}

/// A kind:1 cache-only search request (no relay source installed → cache-only).
fn note_request(query: &str) -> SearchRequest {
    SearchRequest::new(
        query,
        SearchScope::Kinds(BTreeSet::from([NOTE_KIND])),
        SearchTargets::AppDefault,
        Some(50),
    )
    .expect("request")
}

fn decoded_hits(app: &NmpApp, session: &str) -> Vec<nmp_nip50::SearchHit> {
    let bytes = app.search_snapshot_bytes(session).expect("sidecar bytes");
    decode_search_results_snapshot(&bytes)
        .expect("decode N50S")
        .hits
}

/// #1882 regression — the production `open_search` must NOT return cached events
/// that fail the search-TEXT filter. Before the fix, cache hits flowed through
/// the generic observed-interest replay, whose structural gate
/// (`InterestShape::matches_event_with_id`) checks kind + time only — so an
/// unrelated note of the right kind was returned, mislabelled `Relay("")`.
#[test]
fn open_search_excludes_cached_events_that_do_not_match_the_query_text() {
    let app = nmp_app_new();
    // SAFETY: `nmp_app_new` never returns null.
    let app_ref = unsafe { &*app };
    publish_store_with_notes(
        app_ref,
        &[
            (1, "hello nostr world", 100),
            (2, "totally unrelated cooking recipe", 101),
            (3, "another nostr thought", 102),
        ],
    );

    app_ref.open_search(note_request("nostr"), "cache");
    let hits = decoded_hits(app_ref, "cache");

    let ids: BTreeSet<String> = hits.iter().map(|h| h.id.clone()).collect();
    assert_eq!(
        ids,
        BTreeSet::from([note_id(1), note_id(3)]),
        "only the notes whose text matches 'nostr' are returned; the unrelated note (id 2) is excluded"
    );
    // Every cache hit is provenanced `Cache`, never the `Relay(\"\")` the old
    // unfiltered replay produced.
    for hit in &hits {
        assert_eq!(
            hit.source,
            SearchHitSource::Cache,
            "cache hits are Cache-provenanced"
        );
    }

    nmp_app_free(app);
}

/// #1882 regression — a cached event whose content matches the query IS returned
/// as a `Cache` hit (the FTS path is genuinely wired, not just suppressed).
#[test]
fn open_search_returns_text_matching_cache_hit_tagged_cache() {
    let app = nmp_app_new();
    // SAFETY: `nmp_app_new` never returns null.
    let app_ref = unsafe { &*app };
    publish_store_with_notes(app_ref, &[(7, "satoshi nakamoto wrote nostr", 200)]);

    app_ref.open_search(note_request("satoshi"), "match");
    let hits = decoded_hits(app_ref, "match");

    assert_eq!(hits.len(), 1, "the matching cached note is returned");
    assert_eq!(hits[0].id, note_id(7));
    assert_eq!(hits[0].source, SearchHitSource::Cache);
    assert_eq!(hits[0].content, "satoshi nakamoto wrote nostr");

    nmp_app_free(app);
}

/// #1882 — cache↔relay first-arrival dedupe. The cache scan runs synchronously
/// at open, so a cached match keeps its `Cache` tag even when a later relay echo
/// would re-deliver the same id (the projection's first-arrival-wins dedupe is
/// owned + exhaustively tested in `nmp-nip50/src/projection_tests.rs`; here we
/// assert the FFI open path preserves the `Cache` provenance end-to-end through
/// the N50S snapshot). A bare app with no published store is relay-only.
#[test]
fn open_search_cache_only_never_emits_a_relay_provenance() {
    let app = nmp_app_new();
    // SAFETY: `nmp_app_new` never returns null.
    let app_ref = unsafe { &*app };
    publish_store_with_notes(app_ref, &[(1, "nostr cache first", 100)]);

    // No relay source → cache-only fan-out. The sole hit must be Cache.
    app_ref.open_search(note_request("nostr"), "dedupe");
    let hits = decoded_hits(app_ref, "dedupe");
    assert_eq!(hits.len(), 1);
    assert_eq!(
        hits[0].source,
        SearchHitSource::Cache,
        "the synchronous cache hit wins first-arrival; no Relay(\"\") provenance leaks in"
    );

    nmp_app_free(app);
}

#[test]
fn c_abi_open_and_close_are_null_safe() {
    // Null app — no crash.
    let json = CString::new(r#"{"query":"a","scope":"Users","targets":"AppDefault"}"#).unwrap();
    let sid = CString::new("s").unwrap();
    nmp_app_search_open(std::ptr::null_mut(), json.as_ptr(), sid.as_ptr());
    nmp_app_search_close(std::ptr::null_mut(), sid.as_ptr());
    assert_eq!(
        nmp_app_search_snapshot(std::ptr::null_mut(), sid.as_ptr(), std::ptr::null_mut(), 0),
        0
    );

    // Real app, full C-ABI round-trip through the JSON entrypoint.
    let app = nmp_app_new();
    nmp_app_search_open(app, json.as_ptr(), sid.as_ptr());
    // Size-probe: a non-zero size with a null/zero buffer returns the needed len.
    let needed = nmp_app_search_snapshot(app, sid.as_ptr(), std::ptr::null_mut(), 0);
    assert!(needed > 0, "an open session must have an N50S buffer");
    let mut buf = vec![0u8; needed as usize];
    let written = nmp_app_search_snapshot(app, sid.as_ptr(), buf.as_mut_ptr(), buf.len());
    assert_eq!(written, needed);
    assert!(decode_search_results_snapshot(&buf).is_ok());

    nmp_app_search_close(app, sid.as_ptr());
    nmp_app_free(app);
}
