//! NIP-51 list scenarios (S-A03 … S-A05).
//!
//! The list projection is derived from the signed list event's own tags
//! (the relay-free truth) — follow set (kind:30000), bookmarks
//! (kind:30003), and relay list (kind:10002). List kinds have no registered
//! typed projection, so `resolve_embed_projection` emits the `Unknown`
//! variant carrying the raw tags + content tree; native list renderers read
//! those tags directly (no hand-rolled `ListDto` at the fixture layer).

use crate::dto::ScenarioDto;
use crate::embed_store::{EmbedStore, Target};
use crate::identities::{naddr_uri, nevent_uri, Identities};

use super::scenario;

const BASE: u64 = 1_700_040_000;

/// Build every list-category scenario.
pub fn build(ids: &Identities) -> Vec<ScenarioDto> {
    let mut out = Vec::new();

    // S-A03: naddr -> NIP-51 follow set (kind:30000).
    let follow_set = ids.carol.sign(
        30000,
        BASE,
        vec![
            vec!["d".into(), "nostr-core".into()],
            vec!["title".into(), "Nostr Core Devs".into()],
            vec!["p".into(), ids.bob.pubkey_hex.clone()],
            vec!["p".into(), ids.carol.pubkey_hex.clone()],
            vec!["p".into(), ids.eve.pubkey_hex.clone()],
        ],
        "",
    );
    let coord = naddr_uri(30000, &ids.carol.pubkey_hex, "nostr-core");
    let mut store = EmbedStore::default();
    store.add(coord.clone(), Target::Event(follow_set.clone()));
    let e = ids
        .alice
        .sign(1, BASE + 1, vec![], format!("curated devs: {coord}"));
    out.push(scenario(
        "S-A03",
        "lists",
        "naddr -> NIP-51 follow set (kind:30000)",
        "addressable list resolution -> inline titled list card",
        &e,
        vec![follow_set.clone()],
        &store,
    ));

    // S-A04: naddr -> NIP-51 bookmarks (kind:30003), mixed e/a/t items.
    let bookmarks = ids.carol.sign(
        30003,
        BASE + 2,
        vec![
            vec!["d".into(), "reading".into()],
            vec!["title".into(), "Reading List".into()],
            vec!["e".into(), "f".repeat(64)],
            vec![
                "a".into(),
                format!("30023:{}:reconnect-strategy", ids.carol.pubkey_hex),
            ],
            vec!["t".into(), "nostr".into()],
        ],
        "",
    );
    let coord = naddr_uri(30003, &ids.carol.pubkey_hex, "reading");
    let mut store = EmbedStore::default();
    store.add(coord.clone(), Target::Event(bookmarks.clone()));
    let e = ids
        .alice
        .sign(1, BASE + 3, vec![], format!("my reading list {coord}"));
    out.push(scenario(
        "S-A04",
        "lists",
        "naddr -> NIP-51 bookmarks (kind:30003, mixed items)",
        "generic addressable list with heterogeneous e/a/t rows",
        &e,
        vec![bookmarks.clone()],
        &store,
    ));

    // S-A05: NIP-51 relay list (kind:10002) referenced inline.
    let relay_list = ids.carol.sign(
        10002,
        BASE + 4,
        vec![
            vec!["title".into(), "Relay List".into()],
            vec!["r".into(), "wss://relay.a.nmp.test".into()],
            vec!["r".into(), "wss://relay.b.nmp.test".into(), "read".into()],
        ],
        "",
    );
    let uri = nevent_uri(&relay_list.id, &ids.carol.pubkey_hex, 10002);
    let mut store = EmbedStore::default();
    store.add(uri.clone(), Target::Event(relay_list.clone()));
    let e = ids
        .alice
        .sign(1, BASE + 5, vec![], format!("caro's relays {uri}"));
    out.push(scenario(
        "S-A05",
        "lists",
        "NIP-51 relay list (kind:10002) referenced inline",
        "replaceable list resolution -> titled relay list w/ markers",
        &e,
        vec![relay_list.clone()],
        &store,
    ));

    out
}
