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

/// The reserved-builder kinds must classify as `ReservedBuilderOnly` and
/// surface the matching builder.
#[test]
fn reserved_builder_kinds_are_classified_reserved() {
    assert_eq!(
        classify_publish_behavior(KIND_PROFILE_METADATA),
        PublishBehavior::ReservedBuilderOnly(ReservedKind::Profile),
        "kind:0 profile is reserved to PublishProfile"
    );
    assert_eq!(
        classify_publish_behavior(KIND_CONTACT_LIST),
        PublishBehavior::ReservedBuilderOnly(ReservedKind::Contacts),
        "kind:3 contacts is reserved to nmp.follow / nmp.unfollow"
    );
    assert_eq!(
        classify_publish_behavior(KIND_BOOKMARK_LIST),
        PublishBehavior::ReservedBuilderOnly(ReservedKind::Bookmarks),
        "kind:10003 bookmarks are reserved to NIP-51 bookmark builders"
    );

    assert_eq!(
        classify_publish_behavior(KIND_PROFILE_METADATA).reserved_builder(),
        Some(ReservedKind::Profile)
    );
    assert_eq!(
        classify_publish_behavior(KIND_CONTACT_LIST).reserved_builder(),
        Some(ReservedKind::Contacts)
    );
    assert_eq!(
        classify_publish_behavior(KIND_BOOKMARK_LIST).reserved_builder(),
        Some(ReservedKind::Bookmarks)
    );
}

/// The reserved-kind rejection messages must match the wording the old guards
/// produced verbatim — `action.rs` callers assert on these substrings and
/// downstream shells surface them, so the strings are a behaviour contract.
#[test]
fn reserved_kind_rejection_messages_are_preserved() {
    // Exact-match the verbatim wording the old `action.rs` literal guards
    // produced — downstream shells surface these strings and the action tests
    // assert on them, so they are a behaviour contract, not a substring hint.
    assert_eq!(
        ReservedKind::Profile.raw_publish_rejection(),
        "use PublishProfile (not PublishRaw) for kind:0 profile updates",
    );
    assert_eq!(
        ReservedKind::Contacts.raw_publish_rejection(),
        "kind:3 contact-list must be modified via nmp.follow / nmp.unfollow, \
         not PublishRaw (the actor owns the follow-list state)",
    );
    assert_eq!(
        ReservedKind::Bookmarks.raw_publish_rejection(),
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

/// Discovery-indexable replaceables route normally but are recorded as
/// discovery-indexable so the policy mirrors the resolver's indexer fan-out.
#[test]
fn discovery_indexable_kinds_are_classified() {
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
/// envelope (gift-wrap kind:1059, sealed kind:14) with `Auto` / empty
/// `Explicit` (`is_explicit_nonempty == false`) is REJECTED; with an explicit
/// non-empty relay set it is ALLOWED. Public/reserved kinds pass routing
/// regardless of target (their relay selection is the resolver's concern).
#[test]
fn private_kinds_require_explicit_relays_for_routing() {
    // Private + Auto/empty → rejected.
    for kind in [KIND_GIFT_WRAP, KIND_CHAT_MESSAGE] {
        let err = validate_publish_routing(kind, false)
            .expect_err("private kind with Auto/empty target must be rejected");
        assert!(
            err.contains(&format!("kind:{kind}")) && err.contains("D10"),
            "rejection must name the kind and cite D10; got: {err}"
        );
        // Private + explicit non-empty → allowed.
        validate_publish_routing(kind, true)
            .expect("private kind WITH an explicit non-empty relay set must be allowed");
    }
}

/// Non-private kinds (notes, profile, contacts, relay lists) pass routing
/// validation with EITHER target — the D10 gate only constrains private kinds.
#[test]
fn non_private_kinds_route_with_any_target() {
    for kind in [
        KIND_SHORT_TEXT_NOTE,
        KIND_REACTION,
        KIND_PROFILE_METADATA,
        KIND_CONTACT_LIST,
        KIND_RELAY_LIST,
        30_023,
    ] {
        validate_publish_routing(kind, false)
            .unwrap_or_else(|e| panic!("kind:{kind} must route with Auto; got: {e}"));
        validate_publish_routing(kind, true)
            .unwrap_or_else(|e| panic!("kind:{kind} must route with Explicit; got: {e}"));
    }
}

/// The shared structural predicate over `PublishTarget` used at every
/// enforcement site, so the "has an explicit relay pin" fact is derived one
/// way everywhere.
#[test]
fn explicit_nonempty_predicate_matches_target_shape() {
    use crate::publish::PublishTarget;
    assert!(!target_is_explicit_nonempty(&PublishTarget::Auto));
    assert!(!target_is_explicit_nonempty(&PublishTarget::Explicit {
        relays: Vec::new()
    }));
    assert!(target_is_explicit_nonempty(&PublishTarget::Explicit {
        relays: vec!["wss://relay.example".to_string()]
    }));
}

#[path = "tests/gate.rs"]
mod gate;
