//! Tests for the higher-order NIP-50 search FFI surface.

use super::*;
use crate::{nmp_app_free, nmp_app_new};
use nmp_nip50::{
    decode_search_results_snapshot, install_search_relay_source, SearchRelaySource, SearchScope,
    SearchTargets,
};
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
    assert!(parse_search_request(
        r#"{"query":"   ","scope":"Users","targets":"AppDefault"}"#
    )
    .is_none());

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
    install_search_relay_source(app_ref, Arc::new(StubSource {
        preferred: vec!["wss://search.nos.lol/".to_string()],
        default: Vec::new(),
    }));

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
    install_search_relay_source(app_ref, Arc::new(StubSource {
        preferred: vec!["wss://search.nos.lol/".to_string()],
        default: Vec::new(),
    }));

    let request =
        SearchRequest::new("nostr", SearchScope::Users, SearchTargets::UserPreferred, None)
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
    let request =
        SearchRequest::new("nostr", SearchScope::Users, SearchTargets::UserPreferred, Some(10))
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
    let req2 =
        SearchRequest::new("nostr", SearchScope::Users, SearchTargets::UserPreferred, Some(10))
            .expect("request");
    app_ref.open_search(req2, "fallback");
    assert_eq!(
        app_ref.search_session_relays("fallback"),
        vec!["wss://app-default.example/".to_string()],
        "UserPreferred with an empty user list must fall back to the app default"
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
    let request =
        SearchRequest::new("nostr", SearchScope::Users, SearchTargets::UserPreferred, None)
            .expect("request");
    let key = app_ref.open_search(request, "s3");
    assert_eq!(key, "nmp.nip50.search.s3");
    assert!(app_ref.search_snapshot_bytes("s3").is_some());
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
