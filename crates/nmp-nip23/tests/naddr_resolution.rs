#![cfg(feature = "long-form")]
//! `ArticleDetailView` resolves a naddr triple `(kind=30023, author, d_tag)`
//! to the right `ArticleRecord`.
//!
//! `NaddrCoord` is the structured form of an `naddr1…` bech32 — the bech32
//! codec lives in `nmp_core::nip19` (`crates/nmp-core/src/nip19.rs`,
//! `NaddrData` + encode/decode). This test exercises the contract the codec
//! hands off: `(pubkey, kind, d_tag) → ArticleRecord`.

use nmp_core::planner::NaddrCoord;
use nmp_core::substrate::{KernelEvent, ViewContext};
use nmp_nip23::{ArticleDetailSpec, ArticleDetailView, KIND_LONG_FORM_ARTICLE};

const ALICE: &str = "alice-pubkey-0000000000000000000000000000000000000000000000000000000";
const BOB: &str = "bob-pubkey-000000000000000000000000000000000000000000000000000000000000";

fn ke(id: &str, author: &str, created_at: u64, d_tag: &str) -> KernelEvent {
    KernelEvent {
        id: id.into(),
        author: author.into(),
        kind: KIND_LONG_FORM_ARTICLE,
        created_at,
        tags: vec![vec!["d".into(), d_tag.into()]],
        content: format!("body of {id}"),
    }
}

#[test]
fn detail_view_returns_the_article_matching_the_naddr_triple() {
    let coord = NaddrCoord {
        pubkey: ALICE.into(),
        kind: KIND_LONG_FORM_ARTICLE,
        d_tag: "target".into(),
    };

    let spec = ArticleDetailSpec { coord: coord.clone() };
    let (mut state, _payload) = ArticleDetailView::open(&ViewContext::default(), spec);

    // The detail view's `dependencies()` declares the full triple — in
    // production the kernel REQ delivers only matching events. Here we feed
    // the matching event directly and confirm the snapshot returns it.
    let target = ke("evt-target", ALICE, 1_000, "target");
    ArticleDetailView::on_event_inserted(&ViewContext::default(), &mut state, &target);

    let payload = ArticleDetailView::snapshot(&ViewContext::default(), &state);
    assert_eq!(payload.source, "decoded");
    let article = &payload.article;
    assert_eq!(article.event_id, "evt-target");
    assert_eq!(article.author, coord.pubkey);
    assert_eq!(article.d_tag, coord.d_tag);
    assert_eq!(article.content, "body of evt-target");
}

#[test]
fn detail_view_holds_newest_after_replacement() {
    // NIP-23 articles are parameterized-replaceable; a new event with the
    // same (author, d_tag) and a newer `created_at` replaces the older one
    // in the detail view's snapshot.
    let coord = NaddrCoord {
        pubkey: ALICE.into(),
        kind: KIND_LONG_FORM_ARTICLE,
        d_tag: "intro".into(),
    };
    let spec = ArticleDetailSpec { coord: coord.clone() };
    let (mut state, _) = ArticleDetailView::open(&ViewContext::default(), spec);

    let original = ke("evt-old", ALICE, 100, "intro");
    let revision = ke("evt-new", ALICE, 200, "intro");
    ArticleDetailView::on_event_inserted(&ViewContext::default(), &mut state, &original);
    ArticleDetailView::on_event_inserted(&ViewContext::default(), &mut state, &revision);

    let payload = ArticleDetailView::snapshot(&ViewContext::default(), &state);
    assert_eq!(payload.source, "decoded");
    assert_eq!(
        payload.article.event_id, "evt-new",
        "newer event wins NIP-33 replaceability"
    );
}

#[test]
fn detail_view_dependencies_carry_the_coord_triple() {
    let coord = NaddrCoord {
        pubkey: ALICE.into(),
        kind: KIND_LONG_FORM_ARTICLE,
        d_tag: "intro".into(),
    };
    let deps = ArticleDetailView::dependencies(&ArticleDetailSpec { coord: coord.clone() });
    assert_eq!(deps.kinds, vec![KIND_LONG_FORM_ARTICLE]);
    assert_eq!(deps.authors, vec![ALICE.to_string()]);
    assert_eq!(deps.tag_refs, vec![("d".to_string(), "intro".to_string())]);
}

#[test]
fn detail_view_key_is_the_coord_for_dedup_across_callers() {
    // Two callers asking for the same naddr coordinate must share one
    // ArticleDetailView instance — the kernel key is the coord itself.
    let coord = NaddrCoord {
        pubkey: ALICE.into(),
        kind: KIND_LONG_FORM_ARTICLE,
        d_tag: "intro".into(),
    };
    let key1 = ArticleDetailView::key(&ArticleDetailSpec { coord: coord.clone() });
    let key2 = ArticleDetailView::key(&ArticleDetailSpec { coord: coord.clone() });
    assert_eq!(key1, key2);
}

#[test]
fn detail_view_isolates_to_its_coord_across_authors() {
    // Cross-author isolation (codex review finding #2). The detail view is
    // opened for Alice's `d="intro"` coord. A misrouted same-`d` event from a
    // *different author* (Bob) must NOT enter the view's state — even though
    // the shared accumulator keys only on `(author, d_tag)`. We assert:
    //   1. Bob's insert returns `None` (rejected by the coord filter).
    //   2. The snapshot surfaces Alice's article, never Bob's.
    //   3. Removing Bob's id is a no-op — Bob was never stored at all.
    let coord = NaddrCoord {
        pubkey: ALICE.into(),
        kind: KIND_LONG_FORM_ARTICLE,
        d_tag: "intro".into(),
    };
    let spec = ArticleDetailSpec { coord };
    let (mut state, _) = ArticleDetailView::open(&ViewContext::default(), spec);

    let alices = ke("evt-alice", ALICE, 100, "intro");
    // Bob's event has a NEWER created_at — if the accumulator were not coord
    // scoped it would sort to the front of the snapshot and surface as the
    // wrong article. The coord filter must reject it before it ever lands.
    let bobs = ke("evt-bob", BOB, 999, "intro");

    let delta_alice =
        ArticleDetailView::on_event_inserted(&ViewContext::default(), &mut state, &alices);
    let delta_bob =
        ArticleDetailView::on_event_inserted(&ViewContext::default(), &mut state, &bobs);

    assert!(matches!(
        delta_alice,
        Some(nmp_nip23::ArticleViewDelta::Updated(_))
    ));
    assert!(
        delta_bob.is_none(),
        "Bob's off-coord same-d event must be rejected, not admitted"
    );

    let snap = ArticleDetailView::snapshot(&ViewContext::default(), &state);
    assert_eq!(snap.source, "decoded");
    assert_eq!(
        snap.article.event_id, "evt-alice",
        "the view shows only the requested coord's article"
    );
    assert_eq!(snap.article.author, ALICE);

    // Bob was never stored, so removing his id changes nothing.
    let removed = ArticleDetailView::on_event_removed(
        &ViewContext::default(),
        &mut state,
        &"evt-bob".to_string(),
    );
    assert!(removed.is_none(), "Bob's id was never in the state");
    let after = ArticleDetailView::snapshot(&ViewContext::default(), &state);
    assert_eq!(after.article.event_id, "evt-alice");
    assert_eq!(after.source, "decoded");
}
