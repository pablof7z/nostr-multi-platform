//! Tests for the typed feed-session declaration model (#1740 step 1).
//!
//! Primary-kind validation (which kinds are derived acquisition vs. primary
//! input) is protocol knowledge and now lives in the composition/compiler layer
//! — see `explicit composition` (`compile_feed_params`) and the FFI/WASM boundary
//! decode tests. These tests cover only the protocol-agnostic param model.

use super::*;

// ---------------------------------------------------------------------------
// FeedScope / FeedSourceExpr construction + exhaustiveness.
// ---------------------------------------------------------------------------

#[test]
fn feed_source_expr_variants_construct() {
    let follows = FeedScope::ActiveUserFollows;
    let authors = FeedScope::Authors {
        authors: ["deadbeef".to_string()].into_iter().collect(),
    };
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
    let referrer = FeedScope::Referrer {
        event_id: "abc123".into(),
    };
    let pointer_targets = FeedScope::PointerTargets {
        pointers: Box::new(FeedScope::ActiveUserFollows),
        pointer_kinds: vec![7, 1111],
    };
    let hosted_groups = FeedScope::ActiveUserHostedGroups;
    let custom = FeedScope::CustomPerspectiveId(CustomPerspectiveId("trending".into()));

    let union = FeedScope::Union(Box::new(follows.clone()), Box::new(list.clone()));
    let inter = FeedScope::Intersection(Box::new(contacts.clone()), Box::new(wot.clone()));
    let diff = FeedScope::Difference(Box::new(relays.clone()), Box::new(tag.clone()));

    // Exhaustive match — adding a variant forces this to be revisited.
    for expr in [
        follows,
        authors,
        contacts,
        list,
        wot,
        relays,
        tag,
        referrer,
        pointer_targets,
        hosted_groups,
        custom,
        union,
        inter,
        diff,
    ] {
        assert!(describe(&expr).len() > 0);
    }
}

/// Exhaustive matcher proving [`FeedSourceExpr`] is a closed enum: a new variant
/// would break compilation here.
fn describe(expr: &FeedSourceExpr) -> &'static str {
    match expr {
        FeedSourceExpr::ActiveUserFollows => "active-user-follows",
        FeedSourceExpr::Authors { .. } => "authors",
        FeedSourceExpr::ContactList { .. } => "contact-list",
        FeedSourceExpr::ListMembers { .. } => "list-members",
        FeedSourceExpr::Wot { .. } => "wot",
        FeedSourceExpr::RelaySet { .. } => "relay-set",
        FeedSourceExpr::Tag { .. } => "tag",
        FeedSourceExpr::Referrer { .. } => "referrer",
        FeedSourceExpr::PointerTargets { .. } => "pointer-targets",
        FeedSourceExpr::ActiveUserHostedGroups => "active-user-hosted-groups",
        FeedSourceExpr::Union(..) => "union",
        FeedSourceExpr::Intersection(..) => "intersection",
        FeedSourceExpr::Difference(..) => "difference",
        FeedSourceExpr::CustomPerspectiveId(..) => "custom-perspective",
    }
}

#[test]
fn hosted_group_source_is_not_a_pubkey_list() {
    let scope = FeedScope::ActiveUserHostedGroups;
    assert_eq!(describe(&scope), "active-user-hosted-groups");

    let json = serde_json::to_string(&scope).expect("serialize");
    let back: FeedScope = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(scope, back);
}

#[test]
fn pointer_targets_scope_names_pointer_authors_and_kinds() {
    let scope = FeedScope::PointerTargets {
        pointers: Box::new(FeedScope::ActiveUserFollows),
        pointer_kinds: vec![7, 1111],
    };
    assert_eq!(describe(&scope), "pointer-targets");

    let json = serde_json::to_string(&scope).expect("serialize");
    let back: FeedScope = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(scope, back);
}

#[test]
fn custom_perspective_id_is_an_opaque_string_no_trait_no_closure() {
    // The only way app policy enters is via an opaque id — there is no trait to
    // implement and no closure to pass. This test documents that contract.
    let admission = FeedAdmission::Custom(CustomPerspectiveId("nsfw-filter".into()));
    let order = FeedOrder::Custom(CustomPerspectiveId("engagement".into()));
    let scope = FeedScope::CustomPerspectiveId(CustomPerspectiveId("for-you".into()));
    assert_eq!(
        admission,
        FeedAdmission::Custom(CustomPerspectiveId("nsfw-filter".into()))
    );
    assert_eq!(
        order,
        FeedOrder::Custom(CustomPerspectiveId("engagement".into()))
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
fn authors_scope_is_a_static_set_distinct_from_contact_list() {
    // `Authors` names the authors THEMSELVES (a static, app-resolved set whose
    // OWN timeline the feed renders). `ContactList` names an owner whose FOLLOWS
    // seed the scope. They are distinct variants — never interchangeable.
    let one = "aaaa000000000000000000000000000000000000000000000000000000000001";
    let two = "bbbb000000000000000000000000000000000000000000000000000000000002";
    let authors = FeedScope::Authors {
        authors: [one.to_string(), two.to_string()].into_iter().collect(),
    };
    let contacts = FeedScope::ContactList { owner: one.into() };
    assert_ne!(authors, contacts, "author-set ≠ an owner's follow-set");

    // Round-trips through serde verbatim (the resolved set is carried as data).
    let json = serde_json::to_string(&authors).expect("serialize");
    let back: FeedScope = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(authors, back);

    // An EMPTY author set is REPRESENTABLE in the model (the resolver fail-closes
    // it — the model itself names no policy). It must not equal a populated set.
    let empty = FeedScope::Authors {
        authors: std::collections::BTreeSet::new(),
    };
    assert_ne!(empty, authors);
    let back_empty: FeedScope =
        serde_json::from_str(&serde_json::to_string(&empty).unwrap()).unwrap();
    assert_eq!(empty, back_empty);
}

#[test]
fn feed_window_clamps_into_bounds() {
    assert_eq!(
        FeedWindowPolicy { initial_limit: 0 }.bounded_limit(),
        DEFAULT_FEED_WINDOW_LIMIT
    );
    assert_eq!(
        FeedWindowPolicy {
            initial_limit: MAX_FEED_WINDOW_LIMIT + 1000
        }
        .bounded_limit(),
        MAX_FEED_WINDOW_LIMIT
    );
    assert_eq!(FeedWindowPolicy { initial_limit: 25 }.bounded_limit(), 25);
}

#[test]
fn feed_params_round_trips_through_serde() {
    let params = sample_params(vec![1]);
    let json = serde_json::to_string(&params).expect("serialize");
    let back: FeedParams = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(params, back);
    assert!(json.contains("\"key\""));
    assert!(json.contains("\"item_projection\""));
    assert!(
        !json.contains("\"projection\""),
        "FeedParams must not serialize the retired conflated projection field"
    );
}

#[test]
fn feed_handle_pairs_projection_key_and_opaque_session_id() {
    let handle = FeedHandle {
        projection_key: ProjectionKey::app_owned("test.feed.following").unwrap(),
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
        shape: FeedShape::RootIndexed,
        source: FeedScope::ActiveUserFollows,
        admission: FeedAdmission::All,
        order: FeedOrder::NewestByFeedPosition,
        window: FeedWindowPolicy::default(),
        key: ProjectionKey::app_owned("test.feed.following").unwrap(),
        item_projection: FeedItemProjection::FeedRows,
    }
}
