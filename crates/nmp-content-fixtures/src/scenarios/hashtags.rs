//! Hashtag-rendering scenarios (S-H01 … S-H03).
//!
//! These exercise `Segment::Hashtag` along three independent axes:
//!
//! * `S-H01` — one bare `#hashtag` in a plain `kind:1`. The leading `#` is
//!   stripped and the tag is lowercased.
//! * `S-H02` — multiple hashtags interleaved with literal text and a
//!   non-ascii hashtag, exercising the tokenizer's run-of-text vs hashtag
//!   transitions.
//! * `S-H03` — a hashtag inside a `kind:30023` markdown body, so the
//!   `MarkdownInline::Inline(Segment::Hashtag(_))` path emits a
//!   `WireNode::Hashtag` under a `WireNode::Paragraph`.

use crate::dto::ScenarioDto;
use crate::embed_store::EmbedStore;
use crate::identities::Identities;

use super::scenario;

const BASE: u64 = 1_700_080_000;

/// Build every hashtag-category scenario.
pub fn build(ids: &Identities) -> Vec<ScenarioDto> {
    let mut out = Vec::new();

    // S-H01: single hashtag, plain kind:1.
    let store = EmbedStore::default();
    let e = ids.alice.sign(
        1,
        BASE,
        vec![vec!["t".into(), "nostr".into()]],
        "shipped #Nostr",
    );
    out.push(scenario(
        "S-H01",
        "hashtags",
        "Single hashtag",
        "Segment::Hashtag — leading # stripped, lowercased",
        &e,
        vec![],
        &store,
    ));

    // S-H02: multiple hashtags mixed with text, including a non-ascii tag.
    let store = EmbedStore::default();
    let e = ids.alice.sign(
        1,
        BASE + 1,
        vec![
            vec!["t".into(), "nostr".into()],
            vec!["t".into(), "nip01".into()],
            vec!["t".into(), "café".into()],
        ],
        "morning thoughts #Nostr while debugging #NIP01 from the #café",
    );
    out.push(scenario(
        "S-H02",
        "hashtags",
        "Multiple hashtags mixed with text (incl. non-ascii)",
        "tokenizer run-of-text <-> hashtag transitions; unicode tag preserved",
        &e,
        vec![],
        &store,
    ));

    // S-H03: hashtag inside a markdown article body.
    let store = EmbedStore::default();
    let e = ids.carol.sign(
        30023,
        BASE + 2,
        vec![
            vec!["d".into(), "hashtag-essay".into()],
            vec!["title".into(), "On Hashtags".into()],
            vec!["t".into(), "nostr".into()],
        ],
        "# On Hashtags\n\n\
         Tags like #Nostr remain inline hashtags inside markdown bodies.\n",
    );
    out.push(scenario(
        "S-H03",
        "hashtags",
        "Hashtag inside a markdown article body",
        "MarkdownInline::Inline(Segment::Hashtag) under WireNode::Paragraph",
        &e,
        vec![],
        &store,
    ));

    out
}
