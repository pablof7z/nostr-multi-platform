//! Tests for the typed feed-session declaration model (#1740 step 1).

use super::*;
use std::collections::BTreeSet;

// ---------------------------------------------------------------------------
// Fail-closed primary-kind validation: reject wrappers (6/16) + delete (5).
// ---------------------------------------------------------------------------

#[test]
fn primary_kind_1_is_accepted_and_derives_kind_6() {
    let kinds = validate_primary_kinds([1]).expect("[1] is a valid primary set");
    assert_eq!(kinds, BTreeSet::from([1, 6]));
}

#[test]
fn primary_kind_20_is_accepted_and_derives_kind_16() {
    let kinds = validate_primary_kinds([20]).expect("[20] is a valid primary set");
    assert_eq!(kinds, BTreeSet::from([20, 16]));
}

#[test]
fn primary_kind_30023_is_accepted_and_derives_kind_16() {
    let kinds = validate_primary_kinds([30023]).expect("[30023] is a valid primary set");
    assert_eq!(kinds, BTreeSet::from([30023, 16]));
}

#[test]
fn repost_wrapper_6_is_rejected_as_primary() {
    assert_eq!(
        validate_primary_kinds([1, 6]),
        Err(FeedParamsError::RepostWrapperKind { kind: 6 }),
        "[1,6] must be rejected: kind 6 is derived acquisition, not primary"
    );
}

#[test]
fn generic_repost_wrapper_16_is_rejected_as_primary() {
    assert_eq!(
        validate_primary_kinds([20, 16]),
        Err(FeedParamsError::RepostWrapperKind { kind: 16 }),
        "[20,16] must be rejected: kind 16 is derived acquisition, not primary"
    );
    assert_eq!(
        validate_primary_kinds([30023, 16]),
        Err(FeedParamsError::RepostWrapperKind { kind: 16 }),
        "[30023,16] must be rejected: kind 16 is derived acquisition, not primary"
    );
}

#[test]
fn delete_kind_5_is_rejected_as_primary() {
    // [*, 5] — the delete kind is compiler-derived suppression, never primary.
    assert_eq!(
        validate_primary_kinds([1, KIND_DELETE]),
        Err(FeedParamsError::DeleteKind),
        "[1,5] must be rejected: kind 5 is derived suppression, not primary"
    );
    assert_eq!(
        validate_primary_kinds([20, KIND_DELETE]),
        Err(FeedParamsError::DeleteKind)
    );
    assert_eq!(
        validate_primary_kinds([30023, KIND_DELETE]),
        Err(FeedParamsError::DeleteKind)
    );
    assert_eq!(
        validate_primary_kinds([KIND_DELETE]),
        Err(FeedParamsError::DeleteKind)
    );
}

#[test]
fn empty_primary_set_is_rejected() {
    assert_eq!(
        validate_primary_kinds(std::iter::empty::<u32>()),
        Err(FeedParamsError::EmptyPrimaryKinds)
    );
}

#[test]
fn feed_params_validate_delegates_to_primary_kind_validation() {
    let ok = sample_params(vec![1]);
    assert_eq!(ok.validate_primary_kinds(), Ok(BTreeSet::from([1, 6])));

    let bad = sample_params(vec![1, 6]);
    assert_eq!(
        bad.validate_primary_kinds(),
        Err(FeedParamsError::RepostWrapperKind { kind: 6 })
    );
}

// ---------------------------------------------------------------------------
// FeedScope / PubkeySetExpr construction + exhaustiveness.
// ---------------------------------------------------------------------------

#[test]
fn pubkey_set_expr_variants_construct() {
    let follows = FeedScope::ActiveUserFollows;
    let contacts = FeedScope::ContactList {
        owner: "deadbeef".into(),
    };
    let list = FeedScope::ListMembers {
        list: ListId("mutuals".into()),
    };
    let wot = FeedScope::Wot {
        seed: WotSeed("deadbeef".into()),
        rules: WotRulesId("two-hop".into()),
    };
    let relays = FeedScope::RelaySet {
        relays: RelaySetId("read-set".into()),
    };
    let tag = FeedScope::Tag {
        term: TagTerm("nostr".into()),
    };
    let custom = FeedScope::CustomPerspectiveId(CustomPerspectiveId("trending".into()));

    let union = FeedScope::Union(Box::new(follows.clone()), Box::new(list.clone()));
    let inter = FeedScope::Intersection(Box::new(contacts.clone()), Box::new(wot.clone()));
    let diff = FeedScope::Difference(Box::new(relays.clone()), Box::new(tag.clone()));

    // Exhaustive match — adding a variant forces this to be revisited.
    for expr in [follows, contacts, list, wot, relays, tag, custom, union, inter, diff] {
        assert!(describe(&expr).len() > 0);
    }
}

/// Exhaustive matcher proving [`PubkeySetExpr`] is a closed enum: a new variant
/// would break compilation here.
fn describe(expr: &PubkeySetExpr) -> &'static str {
    match expr {
        PubkeySetExpr::ActiveUserFollows => "active-user-follows",
        PubkeySetExpr::ContactList { .. } => "contact-list",
        PubkeySetExpr::ListMembers { .. } => "list-members",
        PubkeySetExpr::Wot { .. } => "wot",
        PubkeySetExpr::RelaySet { .. } => "relay-set",
        PubkeySetExpr::Tag { .. } => "tag",
        PubkeySetExpr::Union(..) => "union",
        PubkeySetExpr::Intersection(..) => "intersection",
        PubkeySetExpr::Difference(..) => "difference",
        PubkeySetExpr::CustomPerspectiveId(..) => "custom-perspective",
    }
}

#[test]
fn custom_perspective_id_is_an_opaque_string_no_trait_no_closure() {
    // The only way app policy enters is via an opaque id — there is no trait to
    // implement and no closure to pass. This test documents that contract.
    let admission = FeedAdmission::Custom(CustomPerspectiveId("nsfw-filter".into()));
    let ranking = FeedRanking::Custom(CustomPerspectiveId("engagement".into()));
    let scope = FeedScope::CustomPerspectiveId(CustomPerspectiveId("for-you".into()));
    assert_eq!(
        admission,
        FeedAdmission::Custom(CustomPerspectiveId("nsfw-filter".into()))
    );
    assert_eq!(
        ranking,
        FeedRanking::Custom(CustomPerspectiveId("engagement".into()))
    );
    assert_eq!(
        scope,
        FeedScope::CustomPerspectiveId(CustomPerspectiveId("for-you".into()))
    );
}

// ---------------------------------------------------------------------------
// FeedParams / FeedHandle shape + serde round-trip.
// ---------------------------------------------------------------------------

#[test]
fn feed_window_clamps_into_bounds() {
    assert_eq!(
        FeedWindow { initial_limit: 0 }.bounded_limit(),
        DEFAULT_FEED_WINDOW_LIMIT
    );
    assert_eq!(
        FeedWindow {
            initial_limit: MAX_FEED_WINDOW_LIMIT + 1000
        }
        .bounded_limit(),
        MAX_FEED_WINDOW_LIMIT
    );
    assert_eq!(FeedWindow { initial_limit: 25 }.bounded_limit(), 25);
}

#[test]
fn feed_params_round_trips_through_serde() {
    let params = sample_params(vec![1]);
    let json = serde_json::to_string(&params).expect("serialize");
    let back: FeedParams = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(params, back);
}

#[test]
fn feed_handle_pairs_projection_key_and_opaque_session_id() {
    let handle = FeedHandle {
        projection_key: ProjectionKey("nmp.feed.home".into()),
        session_id: FeedSessionId(42),
    };
    let json = serde_json::to_string(&handle).expect("serialize");
    let back: FeedHandle = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(handle, back);
    assert_eq!(handle.session_id, FeedSessionId(42));
}

fn sample_params(primary_kinds: Vec<u32>) -> FeedParams {
    FeedParams {
        primary_kinds,
        acquisition: FeedScope::ActiveUserFollows,
        admission: FeedAdmission::All,
        ranking: FeedRanking::ChronologicalDesc,
        window: FeedWindow::default(),
        projection: ProjectionKey("nmp.feed.home".into()),
    }
}
