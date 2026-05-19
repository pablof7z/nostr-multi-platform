//! Plain `kind:1` text scenarios (S-T01 … S-T10).

use crate::dto::ScenarioDto;
use crate::embed_store::EmbedStore;
use crate::identities::Identities;

use super::scenario;

const BASE: u64 = 1_700_000_000;

/// Build every text-category scenario.
pub fn build(ids: &Identities) -> Vec<ScenarioDto> {
    let a = &ids.alice;
    let store = EmbedStore::default();
    let mut out = Vec::new();

    let e = a.sign(
        1,
        BASE,
        vec![],
        "Just shipped the relay reconnect fix. Feels good.",
    );
    out.push(scenario(
        "S-T01",
        "text",
        "Plain text",
        "tokenizer fast-path -> single Segment::Text",
        &e,
        vec![],
        &store,
    ));

    let e = a.sign(
        1,
        BASE + 1,
        vec![
            vec!["t".into(), "nostr".into()],
            vec!["t".into(), "nip01".into()],
            vec!["t".into(), "zaps".into()],
        ],
        "Debugging #Nostr relays again #NIP01 #zaps",
    );
    out.push(scenario(
        "S-T02",
        "text",
        "Hashtags inline",
        "Segment::Hashtag extraction; leading # stripped, lowercased",
        &e,
        vec![],
        &store,
    ));

    let e = a.sign(
        1,
        BASE + 2,
        vec![],
        "Spec lives at https://github.com/nostr-protocol/nips read it",
    );
    out.push(scenario(
        "S-T03",
        "text",
        "Bare URL (non-media)",
        "Segment::Url — URL not classified as media",
        &e,
        vec![],
        &store,
    ));

    let e = a.sign(
        1,
        BASE + 3,
        vec![],
        "Sunset from the office https://nmp.test/img/sunset.jpg",
    );
    out.push(scenario(
        "S-T04",
        "text",
        "Image URL -> media block",
        "grouper post-pass -> Segment::Media { kind: Image }",
        &e,
        vec![],
        &store,
    ));

    let e = a.sign(
        1,
        BASE + 4,
        vec![],
        "Recording of the postmortem https://nmp.test/v/postmortem.mp4",
    );
    out.push(scenario(
        "S-T05",
        "text",
        "Video URL -> media block",
        "Segment::Media { kind: Video } from .mp4",
        &e,
        vec![],
        &store,
    ));

    let e = a.sign(
        1,
        BASE + 5,
        vec![],
        "Trip photos https://nmp.test/img/a.jpg \
         https://nmp.test/img/b.png https://nmp.test/img/c.webp",
    );
    out.push(scenario(
        "S-T06",
        "text",
        "Media gallery (grouped images)",
        "grouper merges consecutive media URLs into ONE Segment::Media",
        &e,
        vec![],
        &store,
    ));

    let e = a.sign(
        1,
        BASE + 6,
        vec![],
        "https://nmp.test/img/x.png https://nmp.test/v/y.webm \
         https://nmp.test/a/z.mp3",
    );
    out.push(scenario(
        "S-T07",
        "text",
        "Mixed media kinds adjacent",
        "grouper boundary: separate Media segment per kind run",
        &e,
        vec![],
        &store,
    ));

    let e = a.sign(
        1,
        BASE + 7,
        vec![
            vec![
                "emoji".into(),
                "nmp".into(),
                "https://nmp.test/e/nmp.png".into(),
            ],
            vec![
                "emoji".into(),
                "rocket".into(),
                "https://nmp.test/e/rocket.png".into(),
            ],
        ],
        "gm :nmp: ship it :rocket:",
    );
    out.push(scenario(
        "S-T08",
        "text",
        "NIP-30 custom emoji (resolved)",
        "Segment::Emoji { url: Some } resolved from emoji tags",
        &e,
        vec![],
        &store,
    ));

    let e = a.sign(1, BASE + 8, vec![], "missing :ghost: shortcode");
    out.push(scenario(
        "S-T09",
        "text",
        "NIP-30 emoji (unresolved)",
        "Segment::Emoji { url: None } — graceful literal fallback (D1)",
        &e,
        vec![],
        &store,
    ));

    let e = a.sign(
        1,
        BASE + 9,
        vec![],
        "zap me lnbc10u1p3xqsrpp5demoinvoicebody or cashu \
         cashuAeyJ0b2tlbnMiOnt9fQ==",
    );
    out.push(scenario(
        "S-T10",
        "text",
        "Lightning / Cashu invoice tokens (reserved)",
        "Segment::Invoice — detect+emit; wallet UX app-owned (M12)",
        &e,
        vec![],
        &store,
    ));

    out
}
