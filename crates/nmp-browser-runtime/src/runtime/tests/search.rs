use std::collections::BTreeSet;

use nmp_core::substrate::KernelEvent;
use nmp_nip50::{
    close_search as close_nip50_search, decode_search_results_snapshot,
    open_search as open_nip50_search, Nip50SearchHandle, Nip50SearchSession, SearchHitSource,
    SearchRequest, SearchScope, SearchTargets,
};
use nmp_store::{RawEvent, VerifiedEvent};

use super::{start_test_browser_builder, started_handle};

const RELAY: &str = "wss://search.example";

fn open_search(
    handle: &mut crate::BrowserRuntimeHandle,
    request: SearchRequest,
    key: &str,
) -> String {
    let _ = open_search_handle(handle, request, key);
    nmp_nip50::search_projection_key(key)
}

fn open_search_handle(
    handle: &mut crate::BrowserRuntimeHandle,
    request: SearchRequest,
    key: &str,
) -> Nip50SearchHandle {
    open_nip50_search(&*handle, Nip50SearchSession::new(request, key))
}

fn close_search(handle: &mut crate::BrowserRuntimeHandle, key: &str) {
    close_nip50_search(&*handle, &Nip50SearchHandle::for_key(key));
}

#[test]
fn browser_search_session_emits_n50s_results_from_live_relay_hits() {
    let mut handle = started_handle();
    let request = SearchRequest::new(
        "nostr",
        SearchScope::Kinds(BTreeSet::from([nmp_kinds::KIND_SHORT_TEXT_NOTE])),
        SearchTargets::Explicit(vec![RELAY.to_string()]),
        Some(10),
    )
    .expect("valid search request");

    let key = open_search(&mut handle, request, "s1");
    assert_eq!(key, "nmp.nip50.search.s1");

    let opened = handle.pump();
    let outbound = opened
        .outbound
        .iter()
        .map(|frame| frame.text().to_string())
        .collect::<Vec<_>>();
    assert!(
        outbound
            .iter()
            .any(|text| text.contains(r#""search":"nostr""#) && text.contains(r#""kinds":[1]"#)),
        "search_open must fan out a NIP-50 REQ, outbound={outbound:?}"
    );

    handle
        .runtime
        .reducer
        .fire_event_observers_for_test(&KernelEvent {
            id: "11".repeat(32),
            author: "22".repeat(32),
            kind: nmp_kinds::KIND_SHORT_TEXT_NOTE,
            created_at: 42,
            tags: Vec::new(),
            content: "nostr browser search result".to_string(),
            relay_provenance: vec![RELAY.to_string()],
        });

    let payload = search_payload(&mut handle, "nmp.nip50.search.s1");
    let snapshot = decode_search_results_snapshot(&payload).expect("N50S decodes");
    assert_eq!(snapshot.hits.len(), 1);
    assert_eq!(snapshot.hits[0].content, "nostr browser search result");
}

#[test]
fn browser_search_session_emits_n50s_results_from_cache_hits() {
    let mut handle = start_test_browser_builder(
        crate::BrowserAppBuilder::new()
            .in_memory()
            .consume_all_builtin_projections()
            .without_initial_relays()
            .decide_providers(crate::BrowserRunConfig::default()),
    );
    handle
        .event_store_handle()
        .insert(
            VerifiedEvent::from_raw_unchecked(RawEvent {
                id: "33".repeat(32),
                pubkey: "44".repeat(32),
                kind: nmp_kinds::KIND_SHORT_TEXT_NOTE,
                created_at: 42,
                tags: Vec::new(),
                content: "hello from fixture relay".to_string(),
                sig: "55".repeat(64),
            }),
            &RELAY.to_string(),
            42_000,
        )
        .expect("cache event inserted");
    let request = SearchRequest::new(
        "fixture relay",
        SearchScope::Kinds(BTreeSet::from([nmp_kinds::KIND_SHORT_TEXT_NOTE])),
        SearchTargets::Explicit(vec![RELAY.to_string()]),
        Some(10),
    )
    .expect("valid search request");

    open_search(&mut handle, request, "s1");

    let payload = search_payload(&mut handle, "nmp.nip50.search.s1");
    let snapshot = decode_search_results_snapshot(&payload).expect("N50S decodes");
    assert_eq!(snapshot.hits.len(), 1);
    assert_eq!(snapshot.hits[0].content, "hello from fixture relay");
    assert!(matches!(snapshot.hits[0].source, SearchHitSource::Cache));
}

#[test]
fn browser_search_session_close_tears_down_projection_and_lifecycle() {
    let mut handle = started_handle();
    let request = SearchRequest::new(
        "nostr",
        SearchScope::Kinds(BTreeSet::from([nmp_kinds::KIND_SHORT_TEXT_NOTE])),
        SearchTargets::Explicit(vec![RELAY.to_string()]),
        Some(10),
    )
    .expect("valid search request");

    let key = open_search(&mut handle, request, "s1");
    assert_eq!(handle.feed_sessions.live_count(), 1);

    close_search(&mut handle, "s1");

    assert_eq!(handle.feed_sessions.live_count(), 0);
    assert!(
        !has_nonempty_search_payload(&mut handle, &key),
        "close_search must remove the typed N50S sidecar"
    );
    close_search(&mut handle, "s1");
    assert_eq!(handle.feed_sessions.live_count(), 0);
}

#[test]
fn browser_search_session_replace_preserves_new_sidecar() {
    let mut handle = started_handle();
    let first = SearchRequest::new(
        "nostr",
        SearchScope::Kinds(BTreeSet::from([nmp_kinds::KIND_SHORT_TEXT_NOTE])),
        SearchTargets::Explicit(vec![RELAY.to_string()]),
        Some(10),
    )
    .expect("valid search request");
    let second = SearchRequest::new(
        "relay",
        SearchScope::Kinds(BTreeSet::from([nmp_kinds::KIND_SHORT_TEXT_NOTE])),
        SearchTargets::Explicit(vec![RELAY.to_string()]),
        Some(10),
    )
    .expect("valid search request");

    let first_handle = open_search_handle(&mut handle, first, "s1");
    let key = nmp_nip50::search_projection_key("s1");
    assert!(has_nonempty_search_payload(&mut handle, &key));

    let _second_handle = open_search_handle(&mut handle, second, "s1");
    assert_eq!(handle.feed_sessions.live_count(), 1);
    close_nip50_search(&handle, &first_handle);
    assert_eq!(handle.feed_sessions.live_count(), 1);
    assert!(
        has_nonempty_search_payload(&mut handle, &key),
        "stale typed close must not remove the replacement N50S sidecar"
    );

    close_search(&mut handle, "s1");
    assert_eq!(handle.feed_sessions.live_count(), 0);
}

#[test]
fn browser_search_session_empty_relays_stays_cache_only_fail_closed() {
    let mut handle = started_handle();
    let request = SearchRequest::new(
        "nostr",
        SearchScope::Kinds(BTreeSet::from([nmp_kinds::KIND_SHORT_TEXT_NOTE])),
        SearchTargets::Explicit(Vec::new()),
        Some(10),
    )
    .expect("valid search request");

    open_search(&mut handle, request, "empty");

    assert_eq!(
        handle.feed_sessions.live_count(),
        1,
        "cache-only search stays closeable through the shared read registry"
    );
    let opened = handle.pump();
    assert!(
        opened.outbound.is_empty(),
        "empty search relay resolution must not open wildcard relay demand"
    );
}

fn search_payload(handle: &mut crate::BrowserRuntimeHandle, key: &str) -> Vec<u8> {
    let bytes = handle
        .produce_snapshot_bytes(true)
        .expect("snapshot frame bytes");
    nmp_core::decode_snapshot_typed_projections(&bytes)
        .expect("typed projections decode")
        .into_iter()
        .find(|row| row.key == key)
        .expect("search projection row present")
        .payload
}

fn has_nonempty_search_payload(handle: &mut crate::BrowserRuntimeHandle, key: &str) -> bool {
    let bytes = handle
        .produce_snapshot_bytes(true)
        .expect("snapshot frame bytes");
    nmp_core::decode_snapshot_typed_projections(&bytes)
        .expect("typed projections decode")
        .into_iter()
        .any(|row| row.key == key && !row.payload.is_empty())
}
