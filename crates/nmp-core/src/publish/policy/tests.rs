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

// ─── REGRESSION GATE — the one-door is enforced, not just declared ──────────

/// The publish routing surface that must NOT contain a raw kind-policy
/// comparison. The classification table (`policy.rs`) is the only legal home
/// for a `kind == <literal>` / `kind == KIND_<reserved|private>` guard; every
/// other file on the publish path must consult the table instead.
const PUBLISH_ROUTING_SURFACE: &[(&str, &str)] = &[
    ("publish/action.rs", include_str!("../action.rs")),
    (
        "actor/commands/publish.rs",
        include_str!("../../actor/commands/publish.rs"),
    ),
    (
        "kernel/publish_cmd.rs",
        include_str!("../../kernel/publish_cmd.rs"),
    ),
    (
        "kernel/publish_engine.rs",
        include_str!("../../kernel/publish_engine.rs"),
    ),
];

/// Kind-policy constants that, used as a `==`/`!=` routing guard, are the
/// scattered-literal anti-pattern this gate bans (reserved-builder + private
/// envelope kinds — the policy-bearing ones). A guard like
/// `raw.kind == KIND_GIFT_WRAP` re-introduces the bug blocker #2 had.
const BANNED_GUARD_CONSTANTS: &[&str] = &[
    "KIND_GIFT_WRAP",
    "KIND_CHAT_MESSAGE",
    "KIND_PROFILE_METADATA",
    "KIND_CONTACT_LIST",
];

/// Returns the offending snippet if a code line compares a `kind` expression
/// against a raw integer or a banned policy constant — the scattered
/// kind-policy guard shape. Shared by the live gate and its non-vacuity proof.
fn kind_policy_guard_violation(code_line: &str) -> Option<String> {
    let normalized = code_line.replace(' ', "");
    // Shape A: a `kind` expression compared to a numeric literal, e.g.
    // `kind==0`, `raw.kind==3`, `.kind==1059`, `kind!=14`. We look for the
    // comparison operator immediately preceded by `kind` and followed by a
    // digit.
    for op in ["kind==", "kind!="] {
        if let Some(idx) = normalized.find(op) {
            let rest = &normalized[idx + op.len()..];
            if rest.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return Some(format!("{op}<int> in `{}`", code_line.trim()));
            }
            // Shape B: a `kind` expression compared to a banned policy
            // constant, e.g. `kind==KIND_GIFT_WRAP`.
            for c in BANNED_GUARD_CONSTANTS {
                if rest.starts_with(c) {
                    return Some(format!("{op}{c} in `{}`", code_line.trim()));
                }
            }
        }
    }
    None
}

/// THE GATE. Every file on the publish routing surface must be free of
/// scattered kind-policy guards — the only place a publish kind may be
/// compared to a literal/policy constant is `policy.rs` (the classification
/// table). This catches blocker #2 (the old `raw.kind == KIND_GIFT_WRAP`
/// guard) and any future reintroduction on ANY publish path, not just
/// `action.rs`.
#[test]
fn publish_routing_surface_has_no_scattered_kind_policy_guards() {
    for (file, src) in PUBLISH_ROUTING_SURFACE {
        for (lineno, line) in src.lines().enumerate() {
            let code = strip_comment(line);
            if let Some(violation) = kind_policy_guard_violation(code) {
                panic!(
                    "{file}:{} reintroduces a scattered kind-policy guard ({violation}). \
                     Route the decision through \
                     `publish::policy::classify_publish_behavior` / \
                     `validate_publish_routing` instead — the classification table is \
                     the ONE door for kind→publish-policy (Workstream C).",
                    lineno + 1
                );
            }
        }
    }
}

/// NON-VACUITY PROOF for the gate above. The detector MUST fire on the exact
/// shapes blocker #2 / the old guards used — if a future edit weakens
/// `kind_policy_guard_violation` into a no-op, this test fails, so the live
/// gate can never silently pass on a real violation.
#[test]
fn gate_detector_fires_on_known_violation_shapes() {
    // The literal guards this PR removed:
    assert!(kind_policy_guard_violation("if kind == 0 {").is_some());
    assert!(kind_policy_guard_violation("if kind == 3 {").is_some());
    // Blocker #2's exact shape:
    assert!(
        kind_policy_guard_violation("if raw.kind == KIND_GIFT_WRAP && matches!(target, ..) {")
            .is_some(),
        "the gate MUST catch the `raw.kind == KIND_GIFT_WRAP` guard (blocker #2)"
    );
    assert!(kind_policy_guard_violation("signed.unsigned.kind == 1059").is_some());
    assert!(kind_policy_guard_violation("kind != 14").is_some());
    // And it must NOT fire on the legal shapes the routing files DO use:
    assert!(
        kind_policy_guard_violation("validate_publish_routing(kind, explicit)").is_none(),
        "consulting the policy table is not a violation"
    );
    assert!(
        kind_policy_guard_violation("classify_publish_behavior(raw.kind)").is_none(),
        "consulting the policy table is not a violation"
    );
    assert!(
        kind_policy_guard_violation("let kind = signed.unsigned.kind;").is_none(),
        "binding a kind value is not a comparison guard"
    );
}

/// Strip a trailing line comment so the gate scans code, not prose. A `//`
/// inside a string literal is rare on a guard line and would only ever cause a
/// false *negative* on the comment tail, never a false positive on code.
fn strip_comment(line: &str) -> &str {
    line.split("//").next().unwrap_or(line)
}
