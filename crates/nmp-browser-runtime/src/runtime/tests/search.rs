use std::collections::BTreeSet;

use nmp_core::substrate::KernelEvent;
use nmp_nip50::{
    decode_search_results_snapshot, SearchHitSource, SearchRequest, SearchScope, SearchTargets,
};
use nmp_store::{RawEvent, VerifiedEvent};

use super::started_handle;

const RELAY: &str = "wss://search.example";

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

    let key = handle.open_search(request, "s1");
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
    let builder = crate::BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections();
    nmp_nip50::register_search_scopes(&builder);
    let mut handle = builder
        .without_initial_relays()
        .decide_providers(crate::BrowserRunConfig::default())
        .start();
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

    handle.open_search(request, "s1");

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

    let key = handle.open_search(request, "s1");
    assert_eq!(handle.search_sessions.live_count(), 1);
    assert_eq!(
        handle.search_sessions.projection_key("s1").as_deref(),
        Some(key.as_str())
    );
    assert_eq!(handle.search_sessions.relays("s1"), vec![RELAY.to_string()]);

    handle.close_search("s1");

    assert_eq!(handle.search_sessions.live_count(), 0);
    assert!(
        !has_nonempty_search_payload(&mut handle, &key),
        "close_search must remove the typed N50S sidecar"
    );
    handle.close_search("s1");
    assert_eq!(handle.search_sessions.live_count(), 0);
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

    handle.open_search(request, "empty");

    assert_eq!(handle.search_sessions.relays("empty"), Vec::<String>::new());
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
