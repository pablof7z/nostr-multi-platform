//! `ArticleDetailView` resolves a naddr triple `(kind=30023, author, d_tag)`
//! to the right `ArticleRecord`.
//!
//! `NaddrCoord` is the structured form of an `naddr1…` bech32 — the bech32
//! codec itself lives in the future `nmp-nip19` crate (see
//! `crates/nmp-core/src/planner/interest.rs:60-63`). This test exercises the
//! contract the codec will hand off: `(pubkey, kind, d_tag) → ArticleRecord`.

use nmp_core::planner::NaddrCoord;
use nmp_core::substrate::{KernelEvent, ViewContext, ViewModule};
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
    let article = payload.article.expect("detail view found the article");
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
    let article = payload.article.expect("a record is present after replacement");
    assert_eq!(article.event_id, "evt-new", "newer event wins NIP-33 replaceability");
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
fn detail_view_independent_authors_do_not_collide_on_d_tag() {
    // Same `d_tag` but different authors are distinct articles. If the
    // accumulator keyed on `d_tag` alone, alice's and bob's d="intro"
    // articles would shadow each other and the snapshot delta for the
    // second insert would be `None` (a no-op replace). We verify the
    // SECOND insert produces an `Updated` delta — proving the accumulator
    // keys on the `(author, d_tag)` tuple, not just `d_tag`.
    let coord = NaddrCoord {
        pubkey: ALICE.into(),
        kind: KIND_LONG_FORM_ARTICLE,
        d_tag: "intro".into(),
    };
    let spec = ArticleDetailSpec { coord };
    let (mut state, _) = ArticleDetailView::open(&ViewContext::default(), spec);

    let alices = ke("evt-alice", ALICE, 100, "intro");
    let bobs = ke("evt-bob", BOB, 200, "intro");

    let delta_alice =
        ArticleDetailView::on_event_inserted(&ViewContext::default(), &mut state, &alices);
    let delta_bob =
        ArticleDetailView::on_event_inserted(&ViewContext::default(), &mut state, &bobs);

    // If d_tag-only keying caused a collision, bob's insert would either
    // replace alice's (still `Some`, but then only one record exists) or be
    // dropped. We assert both inserts registered as updates AND the snapshot
    // surfaces bob's (newest created_at) without having destroyed alice's
    // entry — confirmed by removing bob and seeing alice resurface.
    assert!(matches!(
        delta_alice,
        Some(nmp_nip23::ArticleViewDelta::Updated(_))
    ));
    assert!(matches!(
        delta_bob,
        Some(nmp_nip23::ArticleViewDelta::Updated(_))
    ));

    let after_both = ArticleDetailView::snapshot(&ViewContext::default(), &state)
        .article
        .expect("a record present");
    assert_eq!(after_both.event_id, "evt-bob", "newest created_at wins sort");

    // Remove bob; alice's independent record must still be there — proves it
    // was never shadowed by bob's same-d_tag insert.
    ArticleDetailView::on_event_removed(
        &ViewContext::default(),
        &mut state,
        &"evt-bob".to_string(),
    );
    let after_bob_removed = ArticleDetailView::snapshot(&ViewContext::default(), &state)
        .article
        .expect("alice's record survived bob's same-d_tag insert");
    assert_eq!(after_bob_removed.event_id, "evt-alice");
    assert_eq!(after_bob_removed.author, ALICE);
}
