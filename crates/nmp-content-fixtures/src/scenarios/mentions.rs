//! Profile-mention scenarios (S-M01 … S-M03).

use crate::dto::ScenarioDto;
use crate::embed_store::{EmbedStore, Target};
use crate::identities::Identities;

use super::scenario;

const BASE: u64 = 1_700_010_000;

/// Build every mention-category scenario.
pub fn build(ids: &Identities) -> Vec<ScenarioDto> {
    let mut out = Vec::new();

    // S-M01: npub mention, kind:0 resolved (name + picture).
    let mut store = EmbedStore::default();
    store.add(
        ids.bob.npub_uri(),
        Target::Profile {
            name: Some("bob".to_string()),
            picture: Some("https://nmp.test/img/bob.png".to_string()),
        },
    );
    let e = ids.alice.sign(
        1,
        BASE,
        vec![],
        format!("talked with {} about reconnects", ids.bob.npub_uri()),
    );
    out.push(scenario(
        "S-M01",
        "mentions",
        "npub mention chip (kind:0 resolved)",
        "Segment::Mention(Profile) — chip shows name + avatar",
        &e,
        vec![],
        &store,
    ));

    // S-M02: nprofile mention, kind:0 resolved but no picture -> D1
    // identicon placeholder.
    let mut store = EmbedStore::default();
    store.add(
        ids.carol.nprofile_uri(),
        Target::Profile {
            name: Some("carol".to_string()),
            picture: None,
        },
    );
    let e = ids.alice.sign(
        1,
        BASE + 1,
        vec![],
        format!("shoutout {}", ids.carol.nprofile_uri()),
    );
    out.push(scenario(
        "S-M02",
        "mentions",
        "nprofile mention chip (no picture -> D1 identicon)",
        "Segment::Mention(Profile w/ relay hint); identicon fallback",
        &e,
        vec![],
        &store,
    ));

    // S-M03: mention with NO kind:0 at all -> D1 identicon + npub label.
    let store = EmbedStore::default();
    let e = ids.alice.sign(
        1,
        BASE + 2,
        vec![],
        format!("ping {}", ids.dave.npub_uri()),
    );
    out.push(scenario(
        "S-M03",
        "mentions",
        "Mention with NO kind:0 (D1 identicon placeholder)",
        "D1 best-effort — absent target -> deterministic identicon",
        &e,
        vec![],
        &store,
    ));

    out
}
