//! Quoted-event scenarios (S-M04 … S-M09).
//!
//! Quote chains are signed **deepest-first**: the leaf note (no quotes) is
//! signed first so its id is known, then each parent embeds the child's
//! `nostr:` URI before signing. The A->B->A cycle (S-M09) uses addressable
//! kind:30023 events because their `naddr` coordinate is fully determined
//! before signing (a note-id cycle would require a sha256 fixed point).

use crate::dto::ScenarioDto;
use crate::embed_store::{EmbedStore, Target};
use crate::identities::{naddr_uri, nevent_uri, note_uri, Identities};

use super::scenario;

const BASE: u64 = 1_700_020_000;

/// Build every quote-category scenario.
pub fn build(ids: &Identities) -> Vec<ScenarioDto> {
    let mut out = Vec::new();

    // S-M04: inline quoted note (nostr:note1…).
    let bob_note = ids.bob.sign(
        1,
        BASE,
        vec![],
        "Relays are a CDN you forgot you were running.",
    );
    let mut store = EmbedStore::default();
    store.add(note_uri(&bob_note.id), Target::Event(bob_note.clone()));
    let e = ids.alice.sign(
        1,
        BASE + 1,
        vec![],
        format!("this nails it: {}", note_uri(&bob_note.id)),
    );
    out.push(scenario(
        "S-M04",
        "quotes",
        "Inline quoted note (note1)",
        "Segment::EventRef(Note) -> embedded quoted-note card",
        &e,
        vec![bob_note.clone()],
        &store,
    ));

    // S-M05: same target via nevent1 (relay hint + author + kind).
    let mut store = EmbedStore::default();
    let uri = nevent_uri(&bob_note.id, &ids.bob.pubkey_hex, 1);
    store.add(uri.clone(), Target::Event(bob_note.clone()));
    let e = ids.alice.sign(
        1,
        BASE + 2,
        vec![],
        format!("context: {uri}"),
    );
    out.push(scenario(
        "S-M05",
        "quotes",
        "Inline quoted event (nevent1, relay hint)",
        "Segment::EventRef(Nevent) resolves to same card as note1",
        &e,
        vec![bob_note.clone()],
        &store,
    ));

    // S-M06: quoted note that itself contains a mention.
    let carol_note = ids.carol.sign(
        1,
        BASE + 3,
        vec![],
        format!("agree with {} here", ids.bob.npub_uri()),
    );
    let mut store = EmbedStore::default();
    store.add(
        note_uri(&carol_note.id),
        Target::Event(carol_note.clone()),
    );
    store.add(
        ids.bob.npub_uri(),
        Target::Profile {
            name: Some("bob".to_string()),
            picture: Some("https://nmp.test/img/bob.png".to_string()),
        },
    );
    let e = ids.alice.sign(
        1,
        BASE + 4,
        vec![],
        format!("see {}", note_uri(&carol_note.id)),
    );
    out.push(scenario(
        "S-M06",
        "quotes",
        "Quoted note containing a mention",
        "one level recursion; inner Segment::Mention resolves",
        &e,
        vec![carol_note.clone()],
        &store,
    ));

    // S-M07: nested quotes depth 3 (all < max_depth -> all expand).
    let n_leaf = ids.alice.sign(
        1,
        BASE + 5,
        vec![],
        "Root insight: backpressure is a feature.",
    );
    let n_c = ids.carol.sign(
        1,
        BASE + 6,
        vec![],
        format!("+1 {}", note_uri(&n_leaf.id)),
    );
    let n_b = ids.bob.sign(
        1,
        BASE + 7,
        vec![],
        format!("strongly agree {}", note_uri(&n_c.id)),
    );
    let mut store = EmbedStore::default();
    store.add(note_uri(&n_b.id), Target::Event(n_b.clone()));
    store.add(note_uri(&n_c.id), Target::Event(n_c.clone()));
    store.add(note_uri(&n_leaf.id), Target::Event(n_leaf.clone()));
    let e = ids.alice.sign(
        1,
        BASE + 8,
        vec![],
        format!("thread of thought {}", note_uri(&n_b.id)),
    );
    out.push(scenario(
        "S-M07",
        "quotes",
        "Nested quotes (depth 3, all expand)",
        "multi-level recursion under max_depth=4",
        &e,
        vec![n_b.clone(), n_c.clone(), n_leaf.clone()],
        &store,
    ));

    // S-M08: depth-5 chain -> 5th level collapses (PD-015). Sign
    // deepest-first: N5, N4, N3, N2, N1, then ALICE root.
    let n5 = ids.alice.sign(1, BASE + 9, vec![], "L5 deepest leaf");
    let n4 = ids.bob.sign(
        1,
        BASE + 10,
        vec![],
        format!("L4 {}", note_uri(&n5.id)),
    );
    let n3 = ids.carol.sign(
        1,
        BASE + 11,
        vec![],
        format!("L3 {}", note_uri(&n4.id)),
    );
    let n2 = ids.eve.sign(
        1,
        BASE + 12,
        vec![],
        format!("L2 {}", note_uri(&n3.id)),
    );
    let n1 = ids.bob.sign(
        1,
        BASE + 13,
        vec![],
        format!("L1 {}", note_uri(&n2.id)),
    );
    let mut store = EmbedStore::default();
    for ev in [&n1, &n2, &n3, &n4, &n5] {
        store.add(note_uri(&ev.id), Target::Event(ev.clone()));
    }
    let e = ids.alice.sign(
        1,
        BASE + 14,
        vec![],
        format!("L0 root {}", note_uri(&n1.id)),
    );
    out.push(scenario(
        "S-M08",
        "quotes",
        "Recursion depth >= 4 -> collapse (PD-015)",
        "RenderContext::should_collapse when depth >= max_depth",
        &e,
        vec![
            n1.clone(),
            n2.clone(),
            n3.clone(),
            n4.clone(),
            n5.clone(),
        ],
        &store,
    ));

    // S-M09: cycle A->B->A via addressable kind:30023 (naddr coords are
    // fixed before signing, so each body can reference the other).
    let a_coord = naddr_uri(30023, &ids.alice.pubkey_hex, "cycle-a");
    let b_coord = naddr_uri(30023, &ids.eve.pubkey_hex, "cycle-b");
    let art_a = ids.alice.sign(
        30023,
        BASE + 15,
        vec![
            vec!["d".into(), "cycle-a".into()],
            vec!["title".into(), "Cycle A".into()],
        ],
        format!("# Cycle A\n\nSee the other side: {b_coord}\n"),
    );
    let art_b = ids.eve.sign(
        30023,
        BASE + 16,
        vec![
            vec!["d".into(), "cycle-b".into()],
            vec!["title".into(), "Cycle B".into()],
        ],
        format!("# Cycle B\n\nBack to the start: {a_coord}\n"),
    );
    let mut store = EmbedStore::default();
    store.add(a_coord.clone(), Target::Event(art_a.clone()));
    store.add(b_coord.clone(), Target::Event(art_b.clone()));
    let e = ids.alice.sign(
        1,
        BASE + 17,
        vec![],
        format!("recursive pair: {a_coord}"),
    );
    out.push(scenario(
        "S-M09",
        "quotes",
        "Cycle A->B->A -> visited-set collapse",
        "RenderContext.visited cycle guard (naddr coords)",
        &e,
        vec![art_a.clone(), art_b.clone()],
        &store,
    ));

    out
}
