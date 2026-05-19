//! NIP-23 article scenarios (S-A01, S-A02).
//!
//! Article headers are projected through the **real** `nmp_nip23`
//! decode path (`try_from_kernel_event`); the body is rendered through the
//! **real** `nmp_content::tokenize_with_kind(kind=30023)` Markdown path.

use nmp_core::substrate::{KernelEvent, SignedEvent};

use crate::dto::{ArticleHeaderDto, ScenarioDto};
use crate::embed_store::{EmbedStore, Target};
use crate::identities::{naddr_uri, Identities};

use super::scenario;

const BASE: u64 = 1_700_030_000;

/// Body exercising every PD-012-allowed CommonMark-core construct, plus a
/// GFM table + strikethrough as a deliberate **negative control** (these
/// render as literal text under `Options::empty()` — verified).
fn rich_body(bob_npub: &str) -> String {
    format!(
        "# Reconnect Strategy\n\n\
         ## Background\n\n\
         The relay layer needs **exponential backoff** with *jitter*, and \
         ***both*** matter under load.\n\n\
         ### Steps\n\n\
         1. Detect disconnect\n\
         2. Schedule retry\n   - with jitter\n   - capped at 30s\n\
         3. Resume subscriptions\n\n\
         Use `reconnect()` directly:\n\n\
         ```rust\nfn reconnect() {{ /* backoff */ }}\n```\n\n\
         > Backpressure is a feature.\n>\n> > Even nested.\n\n\
         See the [spec](https://nmp.test/spec) and credit {bob_npub}.\n\n\
         ![Figure 1](https://nmp.test/img/figure.png \"Figure 1\")\n\n\
         ---\n\n\
         | a | b |\n| - | - |\n| 1 | 2 |\n\n\
         Also ~~deprecated~~ approach noted.\n"
    )
}

fn article_header(ev: &SignedEvent) -> ArticleHeaderDto {
    let ke = KernelEvent {
        id: ev.id.clone(),
        author: ev.unsigned.pubkey.clone(),
        kind: ev.unsigned.kind,
        created_at: ev.unsigned.created_at,
        tags: ev.unsigned.tags.clone(),
        content: ev.unsigned.content.clone(),
    };
    let rec = nmp_nip23::decode::try_from_kernel_event(&ke)
        .expect("kind:30023 fixture must decode via real nmp-nip23 path");
    ArticleHeaderDto {
        title: rec.title,
        summary: rec.summary,
        author: rec.author,
        d_tag: rec.d_tag,
    }
}

/// Build every article-category scenario.
pub fn build(ids: &Identities) -> Vec<ScenarioDto> {
    let mut out = Vec::new();
    let bob_npub = ids.bob.npub_uri();

    // S-A01: standalone rich kind:30023 article.
    let article = ids.carol.sign(
        30023,
        BASE,
        vec![
            vec!["d".into(), "reconnect-strategy".into()],
            vec!["title".into(), "Reconnect Strategy".into()],
            vec![
                "summary".into(),
                "Exponential backoff with jitter for relay reconnects."
                    .into(),
            ],
            vec!["published_at".into(), BASE.to_string()],
        ],
        rich_body(&bob_npub),
    );
    let store = EmbedStore::default();
    out.push(scenario(
        "S-A01",
        "articles",
        "kind:30023 rich CommonMark article",
        "real nmp-nip23 decode + tokenize_with_kind Markdown (PD-012/D8)",
        &article,
        vec![],
        &store,
    ));

    // S-A02: naddr -> kind:30023 inside a kind:1 -> Medium-like card.
    let coord =
        naddr_uri(30023, &ids.carol.pubkey_hex, "reconnect-strategy");
    let mut store = EmbedStore::default();
    store.add(
        coord.clone(),
        Target::Article {
            event: article.clone(),
            header: article_header(&article),
        },
    );
    let e = ids.alice.sign(
        1,
        BASE + 1,
        vec![],
        format!("must-read: {coord}"),
    );
    out.push(scenario(
        "S-A02",
        "articles",
        "naddr -> kind:30023 preview card (Medium-like)",
        "Segment::EventRef(Naddr) -> compact article preview",
        &e,
        vec![article.clone()],
        &store,
    ));

    out
}
