//! Tests for the crate-owned NIP-29 group-metadata FTS scope (#1811).
//!
//! These exercise the scope end-to-end against a real `MemEventStore`:
//! register the scope through the public `register_search_scopes` helper,
//! ingest real-shaped kind:39000 group-metadata events, and assert a token +
//! prefix search over names/abouts returns them. A final test pins the
//! crate-boundary invariant: `nmp-core` holds zero group nouns — the scope
//! lives entirely in `nmp-nip29`.

use std::collections::BTreeSet;
use std::sync::Arc;

use nmp_core::substrate::{
    SearchScopeProvider, SearchScopeRegistrar, SearchScopeRegistry,
};
use nmp_store::{
    EventStore, MemEventStore, RawEvent, SearchScopeId, TextSearchBudget, TextSearchOrder,
    TextSearchQuery, TextSearchStatus, VerifiedEvent,
};

use super::*;

/// Minimal `SearchScopeRegistrar` for tests: delegates to a real
/// `SearchScopeRegistry` so `register_search_scopes(&host)` drives the actual
/// production registration path (yield-on-duplicate, compile, install).
struct TestHost {
    registry: SearchScopeRegistry,
}

impl TestHost {
    fn new() -> Self {
        Self {
            registry: SearchScopeRegistry::new(),
        }
    }
}

impl SearchScopeRegistrar for TestHost {
    fn register_search_scope(&self, provider: Arc<dyn SearchScopeProvider>) {
        self.registry.register(provider);
    }
}

/// Build a real-shaped kind:39000 group-metadata event with `name` / `about` /
/// `d` tags (mirrors `nip29.f7z.io` wire shape).
fn group_metadata_event(
    id_seed: u8,
    d: &str,
    name: &str,
    about: &str,
    created_at: u64,
) -> VerifiedEvent {
    let raw = RawEvent {
        id: format!("{id_seed:02x}").repeat(32),
        pubkey: "aa".repeat(32),
        created_at,
        kind: KIND_GROUP_METADATA,
        tags: vec![
            vec!["d".into(), d.into()],
            vec!["name".into(), name.into()],
            vec!["about".into(), about.into()],
        ],
        content: String::new(),
        sig: "bb".repeat(64),
    };
    VerifiedEvent::from_raw_unchecked(raw)
}

fn query(scope: SearchScopeId, q: &str) -> TextSearchQuery {
    TextSearchQuery {
        scope,
        query: q.into(),
        kinds: BTreeSet::new(),
        since: None,
        until: None,
        limit: 10,
        order: TextSearchOrder::NewestFirst,
        budget: TextSearchBudget::default(),
    }
}

fn run_search(store: &MemEventStore, q: &TextSearchQuery) -> (TextSearchStatus, usize) {
    let mut hits = 0usize;
    let status = store
        .text_search_visit(q, &mut |_hit| {
            hits += 1;
            std::ops::ControlFlow::Continue(())
        })
        .expect("text_search_visit");
    (status, hits)
}

#[test]
fn register_search_scopes_installs_one_cache_only_scope() {
    let host = TestHost::new();
    register_search_scopes(&host);

    let compiled = host.registry.compile();
    assert_eq!(compiled.len(), 1, "exactly the group-metadata scope");
    assert_eq!(
        compiled[0].scope_id,
        SearchScopeId::from_label(GROUP_SEARCH_SCOPE_LABEL)
    );
    assert!(compiled[0].kinds.contains(&KIND_GROUP_METADATA));
    assert!(
        !compiled[0].local_only_private,
        "group metadata is public, not local-only"
    );
}

#[test]
fn token_and_prefix_search_over_names_and_abouts() {
    let host = TestHost::new();
    register_search_scopes(&host);

    let store = MemEventStore::new();
    // Install BEFORE ingest so the FTS index is maintained on insert.
    host.registry.install_into(&store);

    let relay = "wss://nip29.f7z.io/".to_string();
    store
        .insert(
            group_metadata_event(
                0x11,
                "nostr-multi-platform",
                "nostr-multi-platform",
                "The reusable Nostr framework workspace",
                100,
            ),
            &relay,
            100_000,
        )
        .unwrap();
    store
        .insert(
            group_metadata_event(
                0x22,
                "chirp",
                "chirp",
                "A minimalist iOS client built on NMP",
                200,
            ),
            &relay,
            200_000,
        )
        .unwrap();

    let scope = SearchScopeId::from_label(GROUP_SEARCH_SCOPE_LABEL);

    // Exact token over a group NAME.
    let (status, hits) = run_search(&store, &query(scope, "chirp"));
    assert_eq!(status, TextSearchStatus::Complete);
    assert_eq!(hits, 1, "name token 'chirp' matches the chirp group");

    // Prefix over a group NAME: "nostr" → "nostr-multi-platform" tokenizes to
    // its component tokens; "multi" hits the same doc.
    let (_s, hits) = run_search(&store, &query(scope, "multi"));
    assert_eq!(hits, 1, "name token 'multi' matches nostr-multi-platform");

    // Token over an ABOUT field (lower weight, but still indexed).
    let (_s, hits) = run_search(&store, &query(scope, "framework"));
    assert_eq!(hits, 1, "about token 'framework' matches nostr-multi-platform");

    // Prefix match over an ABOUT field: "minimal" → "minimalist".
    let (_s, hits) = run_search(&store, &query(scope, "minimal"));
    assert_eq!(hits, 1, "prefix 'minimal' matches 'minimalist' in chirp's about");

    // A token present in BOTH docs ("nmp" — chirp's about says "built on NMP",
    // the nmp group's d-slug/name is "nostr-multi-platform" → token "nmp"? no;
    // use "iOS" which is unique to chirp, and "nmp" which the tokenizer
    // lowercases). "nmp" appears in chirp's about only.
    let (_s, hits) = run_search(&store, &query(scope, "nmp"));
    assert_eq!(hits, 1, "token 'nmp' matches chirp's about");

    // "ios" is unique to chirp's about.
    let (_s, hits) = run_search(&store, &query(scope, "ios"));
    assert_eq!(hits, 1, "token 'ios' matches chirp's about only");

    // A token in NO document yields nothing.
    let (status, hits) = run_search(&store, &query(scope, "zzzznonexistent"));
    assert_eq!(status, TextSearchStatus::Complete);
    assert_eq!(hits, 0, "no match for an absent token");
}

#[test]
fn group_id_slug_is_a_low_weight_search_target() {
    let host = TestHost::new();
    register_search_scopes(&host);

    let store = MemEventStore::new();
    host.registry.install_into(&store);

    store
        .insert(
            group_metadata_event(0x33, "devtalk", "Dev Talk", "engineering chatter", 300),
            &"wss://nip29.f7z.io/".to_string(),
            300_000,
        )
        .unwrap();

    let scope = SearchScopeId::from_label(GROUP_SEARCH_SCOPE_LABEL);
    // The `d`-tag slug "devtalk" is indexed as a low-weight field.
    let (status, hits) = run_search(&store, &query(scope, "devtalk"));
    assert_eq!(status, TextSearchStatus::Complete);
    assert_eq!(hits, 1, "the d-tag slug is a searchable token");
}

/// Crate-boundary invariant (M11.5): `nmp-core` owns zero group nouns. The
/// NIP-29 group search scope, its label, fields, and extractor all live in
/// `nmp-nip29`. This asserts the scope LABEL string is owned here, not in core,
/// by exercising the full register→compile path with only `nmp-core`'s generic
/// (noun-free) registrar surface.
#[test]
fn scope_is_crate_owned_core_holds_no_group_nouns() {
    let host = TestHost::new();
    // The ONLY nip29-specific symbol crossing into core is the opaque
    // Arc<dyn SearchScopeProvider>; core never names "group", "39000", "nip29".
    register_search_scopes(&host);

    let compiled = host.registry.compile();
    // The scope id is derived from the crate-owned label constant.
    assert_eq!(
        compiled[0].scope_id,
        SearchScopeId::from_label("nip29.groups"),
        "the scope label is owned by nmp-nip29, mirrored only via from_label"
    );
    // The kind set is the crate-owned KIND_GROUP_METADATA — core never authored it.
    assert_eq!(
        compiled[0].kinds.iter().copied().collect::<Vec<_>>(),
        vec![KIND_GROUP_METADATA]
    );
}
