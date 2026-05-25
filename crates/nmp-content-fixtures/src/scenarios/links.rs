//! Link-rendering scenarios (S-L01 … S-L03).
//!
//! These pin the three observable link shapes a renderer can encounter:
//!
//! * `S-L01` — a plain `https://…` URL in `kind:1` content. Tokenizes to
//!   `Segment::Url` (no media-extension grouping).
//! * `S-L02` — a `[label](href)` markdown link inside a `kind:30023`
//!   article body. Tokenizes through the Markdown path to a
//!   `MarkdownInline::Link { label, href: Some(_) }`, which the wire
//!   projection emits as a `WireNode::Link { children, href: Some(_) }`.
//! * `S-L03` — a `[label](href)` whose `href` cannot be parsed as a URL.
//!   The CommonMark parser does not recognise an unparseable destination as
//!   a link at all: the bracket / paren / label tokens survive as a
//!   sequence of inline `WireNode::Text` runs under the surrounding
//!   paragraph; **no `WireNode::Link` is emitted** (verified via the
//!   committed wire golden). The fixture is kept so cross-platform
//!   decoders see the degradation shape and the renderer never inherits a
//!   phantom "ungrouped link" surface.

use crate::dto::ScenarioDto;
use crate::embed_store::EmbedStore;
use crate::identities::Identities;

use super::scenario;

const BASE: u64 = 1_700_070_000;

/// Build every links-category scenario.
pub fn build(ids: &Identities) -> Vec<ScenarioDto> {
    let mut out = Vec::new();

    // S-L01: plain URL -> Segment::Url.
    let store = EmbedStore::default();
    let e = ids.alice.sign(
        1,
        BASE,
        vec![],
        "spec lives at https://github.com/nostr-protocol/nips",
    );
    out.push(scenario(
        "S-L01",
        "links",
        "Plain URL (non-media)",
        "Segment::Url — bare URL, not classified as media",
        &e,
        vec![],
        &store,
    ));

    // S-L02: markdown link inside an article (kind:30023 -> Markdown mode).
    let store = EmbedStore::default();
    let e = ids.carol.sign(
        30023,
        BASE + 1,
        vec![
            vec!["d".into(), "links".into()],
            vec!["title".into(), "On Links".into()],
        ],
        "# On Links\n\n\
         The spec lives [on GitHub](https://github.com/nostr-protocol/nips).\n",
    );
    out.push(scenario(
        "S-L02",
        "links",
        "Markdown link with valid href",
        "MarkdownInline::Link { href: Some(_) } -> WireNode::Link",
        &e,
        vec![],
        &store,
    ));

    // S-L03: markdown link whose href cannot be parsed -> href: None.
    // `not a url` is whitespace-bearing and never parses as an absolute URL;
    // the wire projection emits `WireNode::Link { href: None }` (D1).
    let store = EmbedStore::default();
    let e = ids.carol.sign(
        30023,
        BASE + 2,
        vec![
            vec!["d".into(), "broken-link".into()],
            vec!["title".into(), "Broken Link".into()],
        ],
        "# Broken Link\n\n\
         See [this thing](not a url) for context.\n",
    );
    out.push(scenario(
        "S-L03",
        "links",
        "Markdown link with no valid href (D1 best-effort)",
        "Unparseable href -> CommonMark emits no Link node; bracket/paren \
         tokens survive as inline WireNode::Text runs under the paragraph",
        &e,
        vec![],
        &store,
    ));

    out
}
