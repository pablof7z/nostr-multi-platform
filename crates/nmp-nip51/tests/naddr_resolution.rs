//! `ListDetailView` resolves a naddr triple `(kind, author, d_tag)` to the
//! right `ListRecord`, including cross-author AND cross-kind isolation.

use nmp_core::planner::NaddrCoord;
use nmp_core::substrate::{KernelEvent, ViewContext, ViewModule};
use nmp_nip51::{ListDetailSpec, ListDetailView, ListViewDelta, KIND_FOLLOW_SETS, KIND_MUTE_LIST};

const ALICE: &str = "alice-pubkey-0000000000000000000000000000000000000000000000000000000";
const BOB: &str = "bob-pubkey-000000000000000000000000000000000000000000000000000000000000";

fn ke(id: &str, kind: u32, author: &str, created_at: u64, d_tag: &str) -> KernelEvent {
    KernelEvent {
        id: id.into(),
        author: author.into(),
        kind,
        created_at,
        tags: vec![
            vec!["d".into(), d_tag.into()],
            vec!["p".into(), format!("pk-{id}")],
        ],
        content: String::new(),
    }
}

#[test]
fn detail_view_returns_the_set_matching_the_naddr_triple() {
    let coord = NaddrCoord {
        pubkey: ALICE.into(),
        kind: KIND_FOLLOW_SETS,
        d_tag: "friends".into(),
    };
    let (mut state, _) = ListDetailView::open(
        &ViewContext::default(),
        ListDetailSpec {
            coord: coord.clone(),
        },
    );

    let target = ke("evt-target", KIND_FOLLOW_SETS, ALICE, 1_000, "friends");
    ListDetailView::on_event_inserted(&ViewContext::default(), &mut state, &target);

    let payload = ListDetailView::snapshot(&ViewContext::default(), &state);
    assert_eq!(payload.source, "decoded");
    assert_eq!(payload.list.event_id, "evt-target");
    assert_eq!(payload.list.author, coord.pubkey);
    assert_eq!(payload.list.d_tag, "friends");
}

#[test]
fn detail_view_holds_newest_after_replacement() {
    let coord = NaddrCoord {
        pubkey: ALICE.into(),
        kind: KIND_FOLLOW_SETS,
        d_tag: "friends".into(),
    };
    let (mut state, _) = ListDetailView::open(&ViewContext::default(), ListDetailSpec { coord });
    ListDetailView::on_event_inserted(
        &ViewContext::default(),
        &mut state,
        &ke("old", KIND_FOLLOW_SETS, ALICE, 100, "friends"),
    );
    ListDetailView::on_event_inserted(
        &ViewContext::default(),
        &mut state,
        &ke("new", KIND_FOLLOW_SETS, ALICE, 200, "friends"),
    );
    let p = ListDetailView::snapshot(&ViewContext::default(), &state);
    assert_eq!(
        p.list.event_id, "new",
        "newer event wins NIP-33 replaceability"
    );
}

#[test]
fn detail_view_isolates_across_authors() {
    // Cross-author isolation: Bob's same-d, newer set must NOT surface.
    let coord = NaddrCoord {
        pubkey: ALICE.into(),
        kind: KIND_FOLLOW_SETS,
        d_tag: "friends".into(),
    };
    let (mut state, _) = ListDetailView::open(&ViewContext::default(), ListDetailSpec { coord });

    let alices = ke("evt-alice", KIND_FOLLOW_SETS, ALICE, 100, "friends");
    let bobs = ke("evt-bob", KIND_FOLLOW_SETS, BOB, 999, "friends");

    let da = ListDetailView::on_event_inserted(&ViewContext::default(), &mut state, &alices);
    let db = ListDetailView::on_event_inserted(&ViewContext::default(), &mut state, &bobs);

    assert!(matches!(da, Some(ListViewDelta::Updated(_))));
    assert!(db.is_none(), "Bob's off-coord set must be rejected");

    let snap = ListDetailView::snapshot(&ViewContext::default(), &state);
    assert_eq!(snap.list.event_id, "evt-alice");
    assert_eq!(snap.list.author, ALICE);
}

#[test]
fn detail_view_isolates_across_kinds() {
    // Cross-kind isolation (this crate's load-bearing difference vs nip23):
    // a mute list (kind 10000) by Alice with d="friends" must NOT surface in a
    // view opened for Alice's FOLLOW SET (kind 30000) d="friends".
    let coord = NaddrCoord {
        pubkey: ALICE.into(),
        kind: KIND_FOLLOW_SETS,
        d_tag: "friends".into(),
    };
    let (mut state, _) = ListDetailView::open(&ViewContext::default(), ListDetailSpec { coord });

    let wrong_kind = ke("evt-mute", KIND_MUTE_LIST, ALICE, 999, "friends");
    let delta = ListDetailView::on_event_inserted(&ViewContext::default(), &mut state, &wrong_kind);
    assert!(
        delta.is_none(),
        "wrong-kind same-author same-d must be rejected"
    );

    let snap = ListDetailView::snapshot(&ViewContext::default(), &state);
    assert_eq!(
        snap.source, "placeholder",
        "no authoritative event admitted"
    );

    // Now the right kind arrives and resolves.
    let right = ke("evt-fs", KIND_FOLLOW_SETS, ALICE, 50, "friends");
    ListDetailView::on_event_inserted(&ViewContext::default(), &mut state, &right);
    let snap2 = ListDetailView::snapshot(&ViewContext::default(), &state);
    assert_eq!(snap2.source, "decoded");
    assert_eq!(snap2.list.event_id, "evt-fs");
}

#[test]
fn detail_view_key_is_the_coord() {
    let coord = NaddrCoord {
        pubkey: ALICE.into(),
        kind: KIND_FOLLOW_SETS,
        d_tag: "friends".into(),
    };
    let k1 = ListDetailView::key(&ListDetailSpec {
        coord: coord.clone(),
    });
    let k2 = ListDetailView::key(&ListDetailSpec {
        coord: coord.clone(),
    });
    assert_eq!(k1, k2);
}
