//! Tests for the cache-side full-text serve path (issue #1811).
//!
//! These exercise [`Kernel::try_cache_serve_search`] and the relay-only
//! fallback / guard-test contract from `queries.rs`:
//!
//! 1. With a registered cache scope covering the shape's kinds, a search shape
//!    is served from the local FTS index (`text_search_visit`) — hits land in
//!    `kernel.events` with NO relay connectivity (proving the path is
//!    store-only: no network work, point 3 + 4).
//! 2. With NO registered scope, the search stays relay-only:
//!    `try_cache_serve_search` returns `false` and feeds nothing.
//! 3. The structural guard (`shape_to_store_queries` is empty for search shapes)
//!    still holds — full-text serve is a SEPARATE path, not a `StoreQuery`.

use super::super::Kernel;
use crate::planner::InterestShape;
use crate::relay::{DEFAULT_VISIBLE_LIMIT};
use nmp_network::role::RelayRole;
use crate::store::{CompiledIndexSpec, SearchField, SearchScopeId, StoredEvent};
use std::collections::BTreeSet;
use std::sync::Arc;

/// A test fixture scope over kind:1 notes that indexes the note content as one
/// field. Mirrors the shape a real `SearchScopeProvider` would compile into,
/// but built directly so the test does not depend on the registry/composition.
fn note_content_scope() -> CompiledIndexSpec {
    let extract = Arc::new(|ev: &StoredEvent| {
        vec![(SearchField::new(0), ev.raw.content.clone())]
    }) as Arc<crate::store::ExtractFn>;
    CompiledIndexSpec {
        scope_id: SearchScopeId::from_label("test.note.content"),
        kinds: BTreeSet::from([1u32]),
        extract,
        local_only_private: false,
    }
}

/// Seed a signed kind:1 note with `content` into the kernel's store via the live
/// ingest path (which also maintains the FTS index), then clear the in-memory
/// `events`/`timeline` caches to simulate a cold serve (store warm, RAM empty).
fn seed_note(kernel: &mut Kernel, keys: &::nostr::Keys, content: &str, ts: u64) -> String {
    use ::nostr::{EventBuilder, Timestamp};
    let ev = EventBuilder::text_note(content)
        .custom_created_at(Timestamp::from(ts))
        .sign_with_keys(keys)
        .expect("sign cannot fail with a generated keypair");
    let id = ev.id.to_hex();
    let json = serde_json::json!({
        "id": id,
        "pubkey": ev.pubkey.to_hex(),
        "created_at": ev.created_at.as_secs(),
        "kind": ev.kind.as_u16(),
        "tags": Vec::<Vec<String>>::new(),
        "content": ev.content.clone(),
        "sig": ev.sig.to_string(),
    });
    kernel.handle_event(RelayRole::Content, "wss://seed.relay/", "seed-sub", &json);
    id
}

fn search_shape(query: &str) -> InterestShape {
    InterestShape {
        kinds: BTreeSet::from([1u32]),
        search: Some(query.to_string()),
        ..Default::default()
    }
}

#[test]
fn search_shape_with_registered_scope_serves_from_cache() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Install the fixture scope into the kernel's store (composition seam).
    kernel
        .event_store_handle()
        .install_search_index_specs(vec![note_content_scope()]);

    let keys = ::nostr::Keys::generate();
    let base_ts = 1_700_000_000u64;
    let matching = seed_note(&mut kernel, &keys, "learning nostr and rust today", base_ts);
    let other = seed_note(&mut kernel, &keys, "an unrelated cooking post", base_ts + 1);

    // Cold the in-memory caches so the serve must come from the store index.
    kernel.events.clear();
    kernel.timeline.clear();
    assert!(kernel.events.is_empty());

    let shape = search_shape("nostr rust");
    let served = kernel.try_cache_serve_search(&shape, /* completion_key */ 42);

    assert!(served, "a registered cache scope covers kind:1 → cache-served");
    assert!(
        kernel.events.contains_key(matching.as_str()),
        "the matching note must be served from the local FTS index with NO relay"
    );
    assert!(
        !kernel.events.contains_key(other.as_str()),
        "a non-matching note must NOT be served"
    );
    assert!(
        kernel.served_interest_shapes.contains(&42),
        "a cache-covered search shape records its completion key"
    );
    // Store-only: the serve queued no continuation work and needed no network.
    assert!(
        !kernel.has_pending_cache_serves(),
        "search serve is a single bounded call — no continuation queued"
    );
}

#[test]
fn search_shape_without_scope_stays_relay_only() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // NO scope installed.

    let keys = ::nostr::Keys::generate();
    seed_note(&mut kernel, &keys, "learning nostr and rust today", 1_700_000_000);
    kernel.events.clear();

    let shape = search_shape("nostr rust");
    let served = kernel.try_cache_serve_search(&shape, 7);

    assert!(
        !served,
        "no registered cache scope → relay-only (caller keeps prior behaviour)"
    );
    assert!(
        kernel.events.is_empty(),
        "relay-only search must NOT feed any event from the cache"
    );
    assert!(
        !kernel.served_interest_shapes.contains(&7),
        "the relay-only fallback does not record completion here — the caller does"
    );
}

#[test]
fn structural_query_mapping_is_empty_for_search_shapes() {
    // The full-text serve path is SEPARATE from `shape_to_store_queries`: a
    // search shape never produces a structural `StoreQuery` (guard for the
    // issue_1517 contract — covered via the search path, relay otherwise).
    let shape = search_shape("nostr rust");
    assert!(
        super::super::queries::shape_to_store_queries(&shape).is_empty(),
        "search shapes produce no structural StoreQuery (served via text_search_visit)"
    );
}

#[test]
fn empty_kinds_search_shape_is_not_cache_covered() {
    // A kinds-wildcard search would fan across whole corpora — refused as an
    // unbounded scan; such a shape stays relay-only even with a scope installed.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel
        .event_store_handle()
        .install_search_index_specs(vec![note_content_scope()]);

    let shape = InterestShape {
        kinds: BTreeSet::new(),
        search: Some("nostr".to_string()),
        ..Default::default()
    };
    assert!(
        !kernel.try_cache_serve_search(&shape, 9),
        "kinds-wildcard search is not cache-covered (no unbounded scan)"
    );
}
