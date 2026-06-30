//! Nested-embed regression scenarios (S-M10 … S-M14) — F-CR-12.
//!
//! Five scenarios that exercise the five cases called out in the issue:
//!   S-M10  one-deep article quote (resolve_embed_projection → Article)
//!   S-M11  A→B→A cycle via naddr (visited-set collapse, highlight variant)
//!   S-M12  depth-limit (5-deep chain exceeds PD-015 max_depth=4)
//!   S-M13  unknown kind embedded (S-E02 unsupported card)
//!   S-M14  highlight embed (kind:9802, NIP-84)
//!
//! Sign order: deepest-first so each event id is known before its parent
//! embeds it. Cycles use naddr coordinates (stable before signing).

use crate::dto::ScenarioDto;
use crate::embed_store::{EmbedStore, Target};
use crate::identities::{naddr_uri, nevent_uri, note_uri, Identities};

use super::scenario;

const BASE: u64 = 1_700_030_000;

/// Build every highlights/nested-embed regression scenario.
pub fn build(ids: &Identities) -> Vec<ScenarioDto> {
    let mut out = Vec::new();

    // ── S-M10: one-deep article quote ──────────────────────────────────────
    // A note that quotes a kind:30023 article. Exercises the
    // resolve_embed_projection → EmbedKindProjection::Article path, NOT just
    // the ContentTreeDto layer.
    let art = ids.carol.sign(
        30023,
        BASE,
        vec![
            vec!["d".into(), "backpressure-is-a-feature".into()],
            vec!["title".into(), "Backpressure Is A Feature".into()],
            vec!["summary".into(), "Why your relay should push back.".into()],
        ],
        "# Backpressure Is A Feature\n\nWhen demand exceeds supply, \
         the honest answer is: slow down.\n",
    );
    let art_uri = naddr_uri(30023, &ids.carol.pubkey_hex, "backpressure-is-a-feature");
    let mut store = EmbedStore::default();
    store.add(art_uri.clone(), Target::Event(art.clone()));
    let e = ids
        .alice
        .sign(1, BASE + 1, vec![], format!("required reading: {art_uri}"));
    out.push(scenario(
        "S-M10",
        "highlights",
        "One-deep article quote (naddr)",
        "resolve_embed_projection dispatches to Article variant via naddr",
        &e,
        vec![art.clone()],
        &store,
    ));

    // ── S-M11: A→B→A cycle via highlight naddr ─────────────────────────────
    // Two kind:9802 highlights that reference each other via naddr.  The
    // visited-set in EmbedStore::walk terminates the transitive walk after
    // the first back-edge; the renderer derives the PD-015 collapse at walk
    // time.
    let h_a_coord = naddr_uri(9802, &ids.alice.pubkey_hex, "hl-cycle-a");
    let h_b_coord = naddr_uri(9802, &ids.bob.pubkey_hex, "hl-cycle-b");
    let hl_a = ids.alice.sign(
        9802,
        BASE + 2,
        vec![
            vec!["d".into(), "hl-cycle-a".into()],
            vec!["context".into(), format!("See also: {h_b_coord}")],
        ],
        "The first leg of the cycle.",
    );
    let hl_b = ids.bob.sign(
        9802,
        BASE + 3,
        vec![
            vec!["d".into(), "hl-cycle-b".into()],
            vec!["context".into(), format!("Back to: {h_a_coord}")],
        ],
        "The second leg of the cycle.",
    );
    let mut store = EmbedStore::default();
    store.add(h_a_coord.clone(), Target::Event(hl_a.clone()));
    store.add(h_b_coord.clone(), Target::Event(hl_b.clone()));
    let e = ids.carol.sign(
        1,
        BASE + 4,
        vec![],
        format!("cyclic highlights: {h_a_coord}"),
    );
    out.push(scenario(
        "S-M11",
        "highlights",
        "Highlight cycle A→B→A (visited-set collapse)",
        "EmbedStore::walk terminates on back-edge; renderer derives collapse",
        &e,
        vec![hl_a.clone(), hl_b.clone()],
        &store,
    ));

    // ── S-M12: depth-limit chain (5 levels → PD-015 collapse) ─────────────
    // A note chain five levels deep.  Level 5 exceeds PD-015 max_depth=4 so
    // the renderer collapses it.  Sign deepest-first.
    let l5 = ids.alice.sign(1, BASE + 5, vec![], "L5 leaf note");
    let l4 = ids
        .bob
        .sign(1, BASE + 6, vec![], format!("L4 {}", note_uri(&l5.id)));
    let l3 = ids
        .carol
        .sign(1, BASE + 7, vec![], format!("L3 {}", note_uri(&l4.id)));
    let l2 = ids
        .eve
        .sign(1, BASE + 8, vec![], format!("L2 {}", note_uri(&l3.id)));
    let l1 = ids
        .bob
        .sign(1, BASE + 9, vec![], format!("L1 {}", note_uri(&l2.id)));
    let mut store = EmbedStore::default();
    for ev in [&l1, &l2, &l3, &l4, &l5] {
        store.add(note_uri(&ev.id), Target::Event(ev.clone()));
    }
    let e = ids.alice.sign(
        1,
        BASE + 10,
        vec![],
        format!("L0 root {}", note_uri(&l1.id)),
    );
    out.push(scenario(
        "S-M12",
        "highlights",
        "Depth-limit chain (5 levels, PD-015 collapses L5)",
        "RenderContext::should_collapse when depth >= max_depth=4",
        &e,
        vec![l1.clone(), l2.clone(), l3.clone(), l4.clone(), l5.clone()],
        &store,
    ));

    // ── S-M13: unknown kind embedded (S-E02 unsupported card) ──────────────
    // A note quoting a kind:40 (IRC-style channel creation) event which has
    // no NMP embed view.  The store should emit collapsed=true,
    // collapse_reason="unsupported".
    let unknown_ev = ids.dave.sign(
        40,
        BASE + 11,
        vec![],
        r#"{"name":"nmp-dev","about":"NMP developers","picture":""}"#,
    );
    let unknown_uri = nevent_uri(&unknown_ev.id, &ids.dave.pubkey_hex, 40);
    let mut store = EmbedStore::default();
    store.add(unknown_uri.clone(), Target::Event(unknown_ev.clone()));
    let e = ids.alice.sign(
        1,
        BASE + 12,
        vec![],
        format!("old irc-style channel: {unknown_uri}"),
    );
    out.push(scenario(
        "S-M13",
        "highlights",
        "Unknown kind:40 embed (S-E02 unsupported card)",
        "event_entry emits collapsed=true, collapse_reason=unsupported for unregistered kinds",
        &e,
        vec![unknown_ev.clone()],
        &store,
    ));

    // ── S-M14: highlight embed (kind:9802, NIP-84) ─────────────────────────
    // A note quoting a kind:9802 highlight.  The highlight points at an
    // article via the `a` tag and carries a `context` snippet.  Exercises
    // the Highlight arm in event_entry (9802 is now in the `known` set).
    let article_coord = naddr_uri(30023, &ids.carol.pubkey_hex, "backpressure-is-a-feature");
    let highlight = ids.bob.sign(
        9802,
        BASE + 13,
        vec![
            vec![
                "a".into(),
                format!("30023:{}:backpressure-is-a-feature", ids.carol.pubkey_hex),
            ],
            vec![
                "context".into(),
                "When demand exceeds supply, the honest answer is: slow down.".into(),
            ],
        ],
        "the honest answer is: slow down",
    );
    let hl_uri = nevent_uri(&highlight.id, &ids.bob.pubkey_hex, 9802);
    let mut store = EmbedStore::default();
    store.add(hl_uri.clone(), Target::Event(highlight.clone()));
    // Also register the referenced article so transitive walk resolves it.
    let referenced_art = ids.carol.sign(
        30023,
        BASE + 14,
        vec![
            vec!["d".into(), "backpressure-is-a-feature".into()],
            vec!["title".into(), "Backpressure Is A Feature".into()],
        ],
        "# Backpressure Is A Feature\n\nWhen demand exceeds supply, slow down.\n",
    );
    store.add(article_coord.clone(), Target::Event(referenced_art.clone()));
    let e = ids.alice.sign(
        1,
        BASE + 15,
        vec![],
        format!("highlighted passage: {hl_uri}"),
    );
    out.push(scenario(
        "S-M14",
        "highlights",
        "Highlight embed (kind:9802, NIP-84)",
        "EmbedStore resolves 9802 as known; emit rendered content tree, not collapsed",
        &e,
        vec![highlight.clone(), referenced_art.clone()],
        &store,
    ));

    out
}
