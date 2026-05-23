//! Edge / fallback scenarios (S-E01 … S-E07).
//!
//! Invariant for every scenario here: never blank, never crash, never a
//! permanent spinner — D1 best-effort degradation only.

use crate::dto::ScenarioDto;
use crate::embed_store::{EmbedStore, Target};
use crate::identities::{naddr_uri, nevent_uri, Identities};

use super::scenario;

const BASE: u64 = 1_700_050_000;

/// Build every edge/fallback-category scenario.
pub fn build(ids: &Identities) -> Vec<ScenarioDto> {
    let mut out = Vec::new();

    // S-E01: malformed bech32 -> stays literal text.
    let store = EmbedStore::default();
    let e = ids.alice.sign(
        1,
        BASE,
        vec![],
        "broken nostr:npub1thisisnotvalidbech32 still readable",
    );
    out.push(scenario(
        "S-E01",
        "fallback",
        "Malformed bech32 entity",
        "tokenizer rejects invalid nostr: token -> literal text",
        &e,
        vec![],
        &store,
    ));

    // S-E02: unknown referenced kind -> neutral unsupported card.
    let track = ids.bob.sign(
        31337,
        BASE + 1,
        vec![vec!["d".into(), "song".into()]],
        "audio track event body",
    );
    let uri = nevent_uri(&track.id, &ids.bob.pubkey_hex, 31337);
    let mut store = EmbedStore::default();
    store.add(uri.clone(), Target::Event(track.clone()));
    let e = ids.alice.sign(
        1,
        BASE + 2,
        vec![],
        format!("listen: {uri}"),
    );
    out.push(scenario(
        "S-E02",
        "fallback",
        "Unknown / unsupported referenced kind",
        "EventRef to kind:31337 -> graceful unsupported card",
        &e,
        vec![track.clone()],
        &store,
    ));

    // S-E03: dangling nevent -> unresolved stub, no spinner.
    let store = EmbedStore::default();
    let dangling = nevent_uri(&"a".repeat(64), &ids.bob.pubkey_hex, 1);
    let e = ids.alice.sign(
        1,
        BASE + 3,
        vec![],
        format!("ghost quote {dangling}"),
    );
    out.push(scenario(
        "S-E03",
        "fallback",
        "Dangling nevent (target not in store)",
        "absent id -> D1 unresolved-embed stub, never a spinner",
        &e,
        vec![],
        &store,
    ));

    // S-E04: profile mention with no kind:0 at all (explicit control).
    let store = EmbedStore::default();
    let e = ids.alice.sign(
        1,
        BASE + 4,
        vec![],
        format!("hello {}", ids.dave.npub_uri()),
    );
    out.push(scenario(
        "S-E04",
        "fallback",
        "Profile mention with no kind:0 metadata",
        "D1 identicon + npub label, never blank",
        &e,
        vec![],
        &store,
    ));

    // S-E05: empty content -> ContentTree::empty path.
    let store = EmbedStore::default();
    let e = ids.alice.sign(1, BASE + 5, vec![], "");
    out.push(scenario(
        "S-E05",
        "fallback",
        "Empty content event",
        "ContentTree::empty — zero segments, explicit placeholder",
        &e,
        vec![],
        &store,
    ));

    // S-E06: article with empty body but valid metadata.
    let store = EmbedStore::default();
    let e = ids.carol.sign(
        30023,
        BASE + 6,
        vec![
            vec!["d".into(), "draft".into()],
            vec!["title".into(), "Draft".into()],
            vec!["summary".into(), "WIP".into()],
        ],
        "",
    );
    out.push(scenario(
        "S-E06",
        "fallback",
        "Article with empty body but valid metadata",
        "kind:30023 empty body -> tokenize_with_kind renders header-only (D8)",
        &e,
        vec![],
        &store,
    ));

    // S-E07: naddr -> list with zero items.
    let empty_set = ids.carol.sign(
        30000,
        BASE + 7,
        vec![
            vec!["d".into(), "empty".into()],
            vec!["title".into(), "Empty Set".into()],
        ],
        "",
    );
    let coord = naddr_uri(30000, &ids.carol.pubkey_hex, "empty");
    let mut store = EmbedStore::default();
    store.add(
        coord.clone(),
        Target::List {
            event: empty_set.clone(),
            list: crate::dto::ListDto {
                title: Some("Empty Set".to_string()),
                rows: vec![],
            },
        },
    );
    let e = ids.alice.sign(
        1,
        BASE + 8,
        vec![],
        format!("empty list {coord}"),
    );
    out.push(scenario(
        "S-E07",
        "fallback",
        "Naddr -> list with zero items",
        "NIP-51 list view with no members -> titled-but-empty card",
        &e,
        vec![empty_set.clone()],
        &store,
    ));

    out
}
