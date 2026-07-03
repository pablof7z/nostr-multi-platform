//! Tests for the publish-policy one-door classification table.
//!
//! Two duties:
//!  1. Behaviour-preservation — the typed classification covers EVERY kind the
//!     old scattered literal guards covered, with no behaviour drift
//!     (table-driven).
//!  2. Regression gate — assert that publish behaviour is driven only by
//!     [`classify_publish_behavior`], so a new raw kind literal added to the
//!     publish path outside this table is caught (source-scan over
//!     `publish/action.rs`).

use super::*;
use crate::kinds::{
    KIND_BLOCKED_RELAYS, KIND_BOOKMARK_LIST, KIND_CHAT_MESSAGE, KIND_CONTACT_LIST,
    KIND_DM_RELAY_LIST, KIND_GIFT_WRAP, KIND_MUTE_LIST, KIND_PROFILE_METADATA, KIND_REACTION,
    KIND_RELAY_LIST, KIND_SHORT_TEXT_NOTE,
};
use std::sync::Once;

static REGISTER_TEST_POLICIES: Once = Once::new();

fn register_test_protocol_policies() {
    REGISTER_TEST_POLICIES.call_once(|| {
        register_reserved_publish_builder(
            KIND_PROFILE_METADATA,
            "use PublishProfile (not PublishRaw) for kind:0 profile updates",
        )
        .expect("kind:0 policy must register");
        register_discovery_indexable_publish_kind(KIND_PROFILE_METADATA);
        register_reserved_publish_builder(
            KIND_CONTACT_LIST,
            "kind:3 contact-list must be modified via nmp.follow / nmp.unfollow, \
             not PublishRaw (the actor owns the follow-list state)",
        )
        .expect("kind:3 policy must register");
        register_discovery_indexable_publish_kind(KIND_CONTACT_LIST);
        register_reserved_publish_builder(
            KIND_BOOKMARK_LIST,
            "kind:10003 bookmark list must be modified via \
             nmp.nip51.add_bookmark / nmp.nip51.remove_bookmark, not PublishRaw \
             (the NIP-51 builder owns the list merge)",
        )
        .expect("kind:10003 policy must register");
        register_discovery_indexable_publish_range(10_000..=19_999);
        register_discovery_indexable_publish_kind(KIND_RELAY_LIST);
        register_discovery_indexable_publish_kind(KIND_DM_RELAY_LIST);
        register_discovery_indexable_publish_kind(KIND_MUTE_LIST);
        register_discovery_indexable_publish_kind(KIND_BLOCKED_RELAYS);
    });
}

/// The reserved-builder kinds must classify as `ReservedBuilderOnly` and
/// surface the matching builder.
#[test]
fn reserved_builder_kinds_are_classified_reserved() {
    register_test_protocol_policies();
    let profile = ReservedBuilderPolicy::new(
        "use PublishProfile (not PublishRaw) for kind:0 profile updates",
    );
    let contacts = ReservedBuilderPolicy::new(
        "kind:3 contact-list must be modified via nmp.follow / nmp.unfollow, \
         not PublishRaw (the actor owns the follow-list state)",
    );
    let bookmarks = ReservedBuilderPolicy::new(
        "kind:10003 bookmark list must be modified via \
         nmp.nip51.add_bookmark / nmp.nip51.remove_bookmark, not PublishRaw \
         (the NIP-51 builder owns the list merge)",
    );
    assert_eq!(
        classify_publish_behavior(KIND_PROFILE_METADATA),
        PublishBehavior::ReservedBuilderOnly(profile),
        "kind:0 profile is reserved to PublishProfile"
    );
    assert_eq!(
        classify_publish_behavior(KIND_CONTACT_LIST),
        PublishBehavior::ReservedBuilderOnly(contacts),
        "kind:3 contacts is reserved to nmp.follow / nmp.unfollow"
    );
    assert_eq!(
        classify_publish_behavior(KIND_BOOKMARK_LIST),
        PublishBehavior::ReservedBuilderOnly(bookmarks),
        "kind:10003 bookmarks are reserved to NIP-51 bookmark builders"
    );

    assert_eq!(
        classify_publish_behavior(KIND_PROFILE_METADATA).reserved_builder(),
        Some(profile)
    );
    assert_eq!(
        classify_publish_behavior(KIND_CONTACT_LIST).reserved_builder(),
        Some(contacts)
    );
    assert_eq!(
        classify_publish_behavior(KIND_BOOKMARK_LIST).reserved_builder(),
        Some(bookmarks)
    );
}

/// The reserved-kind rejection messages must match the wording the old guards
/// produced verbatim — `action.rs` callers assert on these substrings and
/// downstream shells surface them, so the strings are a behaviour contract.
#[test]
fn reserved_kind_rejection_messages_are_preserved() {
    register_test_protocol_policies();
    // Exact-match the verbatim wording the old `action.rs` literal guards
    // produced — downstream shells surface these strings and the action tests
    // assert on them, so they are a behaviour contract, not a substring hint.
    assert_eq!(
        classify_publish_behavior(KIND_PROFILE_METADATA)
            .reserved_builder()
            .expect("kind:0 must be reserved")
            .raw_publish_rejection(),
        "use PublishProfile (not PublishRaw) for kind:0 profile updates",
    );
    assert_eq!(
        classify_publish_behavior(KIND_CONTACT_LIST)
            .reserved_builder()
            .expect("kind:3 must be reserved")
            .raw_publish_rejection(),
        "kind:3 contact-list must be modified via nmp.follow / nmp.unfollow, \
         not PublishRaw (the actor owns the follow-list state)",
    );
    assert_eq!(
        classify_publish_behavior(KIND_BOOKMARK_LIST)
            .reserved_builder()
            .expect("kind:10003 must be reserved")
            .raw_publish_rejection(),
        "kind:10003 bookmark list must be modified via \
         nmp.nip51.add_bookmark / nmp.nip51.remove_bookmark, not PublishRaw \
         (the NIP-51 builder owns the list merge)",
    );
}

/// Private envelope kinds (gift-wrap kind:1059, sealed chat kind:14) classify
/// as `PrivateFailClosed` — they must never be a raw-publish-allowed public
/// kind, which would let `Auto` leak them to public relays (D10).
#[test]
fn private_envelope_kinds_fail_closed() {
    assert_eq!(
        classify_publish_behavior(KIND_GIFT_WRAP),
        PublishBehavior::PrivateFailClosed,
        "kind:1059 gift-wrap must fail closed"
    );
    assert_eq!(
        classify_publish_behavior(KIND_CHAT_MESSAGE),
        PublishBehavior::PrivateFailClosed,
        "kind:14 sealed chat message must fail closed"
    );
    // A private kind is never reserved-builder and never public-routable.
    assert_eq!(
        classify_publish_behavior(KIND_GIFT_WRAP).reserved_builder(),
        None
    );
}

#[test]
fn private_fail_closed_is_hardcoded_ahead_of_registered_policy() {
    register_discovery_indexable_publish_kind(KIND_GIFT_WRAP);
    register_discovery_indexable_publish_kind(KIND_CHAT_MESSAGE);

    assert_eq!(
        classify_publish_behavior(KIND_GIFT_WRAP),
        PublishBehavior::PrivateFailClosed,
        "registered public policy must never downgrade gift-wrap routing"
    );
    assert_eq!(
        classify_publish_behavior(KIND_CHAT_MESSAGE),
        PublishBehavior::PrivateFailClosed,
        "registered public policy must never downgrade sealed chat routing"
    );
}

/// Discovery-indexable replaceables route normally but are recorded as
/// discovery-indexable so the policy mirrors the resolver's indexer fan-out.
#[test]
fn discovery_indexable_kinds_are_classified() {
    register_test_protocol_policies();
    for kind in [
        KIND_RELAY_LIST,
        KIND_DM_RELAY_LIST,
        KIND_MUTE_LIST,
        KIND_BLOCKED_RELAYS,
        10_000,
        10_004,
        12_345,
        19_999,
    ] {
        assert_eq!(
            classify_publish_behavior(kind),
            PublishBehavior::DiscoveryIndexable,
            "kind:{kind} must be discovery-indexable"
        );
        assert_eq!(
            classify_publish_behavior(kind).reserved_builder(),
            None,
            "kind:{kind} is publishable raw (not reserved)"
        );
    }
}

/// Ordinary public kinds (notes, reactions, custom app kinds, addressables)
/// are publicly routable and publishable raw.
#[test]
fn public_kinds_are_routable() {
    register_test_protocol_policies();
    for kind in [
        KIND_SHORT_TEXT_NOTE, // kind:1
        KIND_REACTION,        // kind:7
        9_999,                // upper non-list replaceable
        30_023,               // NIP-23 long-form (addressable)
        39_999,               // upper addressable
        65_000,               // arbitrary custom app kind
    ] {
        assert_eq!(
            classify_publish_behavior(kind),
            PublishBehavior::PublicRoutable,
            "kind:{kind} must be public-routable"
        );
        assert_eq!(
            classify_publish_behavior(kind).reserved_builder(),
            None,
            "kind:{kind} is publishable raw (not reserved)"
        );
    }
}

/// Exactly the reserved kinds (and no others across a wide sweep) gate a raw
/// publish — locks the reserved set so a future edit can't silently widen or
/// shrink it without updating this assertion.
#[test]
fn only_expected_kinds_are_reserved_across_full_sweep() {
    register_test_protocol_policies();
    let mut reserved: Vec<u32> = Vec::new();
    for kind in 0u32..=40_000 {
        if classify_publish_behavior(kind).reserved_builder().is_some() {
            reserved.push(kind);
        }
    }
    assert_eq!(
        reserved,
        vec![KIND_PROFILE_METADATA, KIND_CONTACT_LIST, KIND_BOOKMARK_LIST],
        "exactly kind:0, kind:3, and kind:10003 are reserved-builder-only"
    );
}

// ─── Enforcement of the PrivateFailClosed routing invariant ─────────────────

/// `validate_publish_routing` is the typed one-door routing gate. A private
/// envelope (gift-wrap kind:1059, sealed kind:14) with `Auto`, empty
/// `Explicit`, or an explicit route whose class is not `VerifiedPrivateInbox`
/// is REJECTED; with a verified private inbox relay set it is ALLOWED.
/// Public/reserved kinds pass routing regardless of target (their relay
/// selection is the resolver's concern).
#[test]
fn private_kinds_require_verified_private_inbox_for_routing() {
    use crate::publish::{PublishRouteClass, PublishTarget};

    for kind in [KIND_GIFT_WRAP, KIND_CHAT_MESSAGE] {
        for target in [
            PublishTarget::Auto,
            PublishTarget::explicit(
                vec!["wss://relay.example".to_string()],
                PublishRouteClass::ManualOverride,
            ),
            PublishTarget::explicit(
                vec!["wss://relay.example".to_string()],
                PublishRouteClass::GroupHostPin,
            ),
            PublishTarget::explicit(
                vec!["wss://relay.example".to_string()],
                PublishRouteClass::Diagnostic,
            ),
            PublishTarget::explicit(Vec::new(), PublishRouteClass::VerifiedPrivateInbox),
        ] {
            let err = validate_publish_routing(kind, &target)
                .expect_err("private kind without verified inbox provenance must be rejected");
            assert!(
                err.contains(&format!("kind:{kind}"))
                    && err.contains("verified_private_inbox")
                    && err.contains("D10"),
                "rejection must name the kind, provenance class, and D10; got: {err}"
            );
        }

        validate_publish_routing(
            kind,
            &PublishTarget::explicit(
                vec!["wss://relay.example".to_string()],
                PublishRouteClass::VerifiedPrivateInbox,
            ),
        )
        .expect("private kind with verified private inbox route must be allowed");
    }
}

/// Non-private kinds (notes, profile, contacts, relay lists) pass routing
/// validation with EITHER target — the D10 gate only constrains private kinds.
#[test]
fn non_private_kinds_route_with_any_target() {
    register_test_protocol_policies();
    use crate::publish::{PublishRouteClass, PublishTarget};

    for kind in [
        KIND_SHORT_TEXT_NOTE,
        KIND_REACTION,
        KIND_PROFILE_METADATA,
        KIND_CONTACT_LIST,
        KIND_RELAY_LIST,
        30_023,
    ] {
        validate_publish_routing(kind, &PublishTarget::Auto)
            .unwrap_or_else(|e| panic!("kind:{kind} must route with Auto; got: {e}"));
        validate_publish_routing(
            kind,
            &PublishTarget::explicit(
                vec!["wss://relay.example".to_string()],
                PublishRouteClass::ManualOverride,
            ),
        )
        .unwrap_or_else(|e| panic!("kind:{kind} must route with Explicit; got: {e}"));
    }
}

#[path = "tests/gate.rs"]
mod gate;
