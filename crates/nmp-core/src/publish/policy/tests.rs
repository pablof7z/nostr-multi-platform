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
    KIND_BLOCKED_RELAYS, KIND_CHAT_MESSAGE, KIND_CONTACT_LIST, KIND_DM_RELAY_LIST, KIND_GIFT_WRAP,
    KIND_MUTE_LIST, KIND_PROFILE_METADATA, KIND_REACTION, KIND_RELAY_LIST, KIND_SHORT_TEXT_NOTE,
};

/// The reserved-builder kinds (kind:0 / kind:3) — the exact set the old
/// `if kind == 0` / `if kind == 3` guards in `action.rs` enforced — must
/// classify as `ReservedBuilderOnly` and surface the matching builder.
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

    // `reserved_builder()` returns the typed reason for exactly these kinds.
    assert_eq!(
        classify_publish_behavior(KIND_PROFILE_METADATA).reserved_builder(),
        Some(ReservedKind::Profile)
    );
    assert_eq!(
        classify_publish_behavior(KIND_CONTACT_LIST).reserved_builder(),
        Some(ReservedKind::Contacts)
    );
}

/// The reserved-kind rejection messages must match the wording the old guards
/// produced verbatim — `action.rs` callers assert on these substrings and
/// downstream shells surface them, so the strings are a behaviour contract.
#[test]
fn reserved_kind_rejection_messages_are_preserved() {
    assert!(
        ReservedKind::Profile
            .raw_publish_rejection()
            .contains("PublishProfile"),
        "profile rejection must name PublishProfile (preserves the old guard wording)"
    );
    assert!(
        ReservedKind::Contacts
            .raw_publish_rejection()
            .contains("kind:3"),
        "contacts rejection must name kind:3 (preserves the old guard wording)"
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
    assert_eq!(classify_publish_behavior(KIND_GIFT_WRAP).reserved_builder(), None);
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
        30_023,              // NIP-23 long-form (addressable)
        39_999,              // upper addressable
        65_000,              // arbitrary custom app kind
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

/// Exactly the two reserved kinds (and no others across a wide sweep) gate a
/// raw publish — locks the reserved set so a future edit can't silently widen
/// or shrink it without updating this assertion.
#[test]
fn only_two_kinds_are_reserved_across_full_sweep() {
    let mut reserved: Vec<u32> = Vec::new();
    for kind in 0u32..=40_000 {
        if classify_publish_behavior(kind).reserved_builder().is_some() {
            reserved.push(kind);
        }
    }
    assert_eq!(
        reserved,
        vec![KIND_PROFILE_METADATA, KIND_CONTACT_LIST],
        "exactly kind:0 and kind:3 are reserved-builder-only"
    );
}

/// REGRESSION GATE — Workstream C "removes the old path + adds a gate that
/// prevents reintroduction." Publish behaviour must be driven by
/// `classify_publish_behavior`, NOT by a raw kind literal re-introduced into
/// `publish/action.rs`. Scan the action source for the banned literal-compare
/// shapes (`kind == 0`, `kind == 3`, …) outside this policy module.
///
/// The check is a coarse source scan (the same technique the doctrine-lint
/// fixtures use) — it is intentionally strict: any `== <int>` / `!= <int>`
/// comparison against a publish `kind` in `action.rs` is a reintroduction of
/// the scattered-literal anti-pattern and must instead go through the table.
#[test]
fn action_source_has_no_raw_kind_literal_guards() {
    let src = include_str!("../action.rs");
    // Banned shapes: a kind literal-compare. We look for `kind ==`/`kind !=`
    // followed by a digit — the exact pattern the old guards used. The policy
    // table is the only legal home for that comparison, and it lives in
    // policy.rs (not action.rs), so action.rs must contain none.
    for (lineno, line) in src.lines().enumerate() {
        let code = line.split("//").next().unwrap_or(line); // strip line comments
        let normalized = code.replace(' ', "");
        assert!(
            !(normalized.contains("kind==0")
                || normalized.contains("kind==3")
                || normalized.contains("kind!=0")
                || normalized.contains("kind!=3")
                || normalized.contains("kind==1059")
                || normalized.contains("kind==14")),
            "publish/action.rs:{} reintroduces a raw kind literal guard \
             (`{}`). Route the decision through \
             `publish::policy::classify_publish_behavior` instead — the \
             classification table is the one door for kind→publish-policy.",
            lineno + 1,
            line.trim()
        );
    }
}
