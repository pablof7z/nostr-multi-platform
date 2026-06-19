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
    // The universal per-relay emit gate lives in the engine's `dispatch_due`
    // (engine/helpers.rs) — include it so a reintroduced literal at the very
    // emit site is caught too.
    (
        "publish/engine/helpers.rs",
        include_str!("../engine/helpers.rs"),
    ),
    (
        "publish/engine/dispatch.rs",
        include_str!("../engine/dispatch.rs"),
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
    "KIND_BOOKMARK_LIST",
];

/// The policy-bearing kind integers (reserved-builder + private envelope) that,
/// used as a routing guard in ANY shape — `==`, `match` arm, `matches!`,
/// `.contains` — re-introduce the scattered-literal anti-pattern. The only
/// legal place to compare a publish kind against these is `policy.rs`.
const BANNED_KIND_LITERALS: &[&str] = &["0", "3", "14", "1059", "10003"];

/// Does this code line contain a `kind`-bearing token? (`kind`, `raw.kind`,
/// `signed.unsigned.kind`, `event.unsigned.kind`, …) — used to scope the
/// shape heuristics so a bare integer elsewhere is not flagged.
fn mentions_kind(normalized: &str) -> bool {
    normalized.contains("kind")
}

/// `true` if `rest` begins with a banned kind literal that is NOT immediately
/// followed by another digit (so `1059` matches but `10591` / `30023` do not).
fn starts_with_banned_literal(rest: &str) -> Option<&'static str> {
    for lit in BANNED_KIND_LITERALS {
        if let Some(after) = rest.strip_prefix(*lit) {
            if !after.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return Some(lit);
            }
        }
    }
    None
}

/// Returns the offending snippet if a code line guards on a `kind` expression
/// using a raw integer or a banned policy constant in ANY of the common
/// shapes — `==`/`!=`, `match`, `matches!`, `.contains(` — the scattered
/// kind-policy guard anti-pattern. Shared by the live gate and its
/// non-vacuity proof. Heuristic by design (a line scanner can't be perfect),
/// but it MUST catch the common evasions, proven by
/// `gate_detector_fires_on_*` tests.
fn kind_policy_guard_violation(code_line: &str) -> Option<String> {
    let normalized = code_line.replace(' ', "");

    // Shape A/B: a `kind` expression directly compared, e.g. `kind==0`,
    // `raw.kind==1059`, `kind!=14`, `kind==KIND_GIFT_WRAP`.
    for op in ["kind==", "kind!="] {
        if let Some(idx) = normalized.find(op) {
            let rest = &normalized[idx + op.len()..];
            if let Some(lit) = starts_with_banned_literal(rest) {
                return Some(format!("{op}{lit} in `{}`", code_line.trim()));
            }
            for c in BANNED_GUARD_CONSTANTS {
                if rest.starts_with(c) {
                    return Some(format!("{op}{c} in `{}`", code_line.trim()));
                }
            }
        }
    }

    // Shape C: `match` on a kind expression with a policy literal/constant on
    // the line — `matchkind{`, `matchraw.kind{`, or a match arm `1059=>` /
    // `KIND_GIFT_WRAP=>` that is a kind-policy dispatch. Flag a `match` that
    // names `kind` and carries a banned literal/constant, OR any `=>` arm whose
    // pattern is a banned policy token.
    if normalized.contains("match") && mentions_kind(&normalized) {
        if let Some(found) = banned_token_present(&normalized) {
            return Some(format!("match-on-kind ({found}) in `{}`", code_line.trim()));
        }
    }
    // A standalone match arm `<bannedKind>=>` / `KIND_*=>` (the match head may
    // be on a previous line — catch the arm itself on the routing surface).
    if let Some(idx) = normalized.find("=>") {
        let lhs = &normalized[..idx];
        if let Some(found) = banned_token_present(lhs) {
            // Avoid flagging a numeric arm that is part of a larger number or a
            // non-kind match: require the arm to look like a bare kind literal /
            // constant pattern (`1059=>`, `1059|14=>`, `KIND_GIFT_WRAP=>`).
            if lhs
                .chars()
                .all(|c| c.is_ascii_digit() || c == '|' || c == '_' || c.is_alphabetic())
            {
                return Some(format!("match-arm ({found}) in `{}`", code_line.trim()));
            }
        }
    }

    // Shape D: `matches!(<kind-expr>, <banned literal/constant>)`.
    if normalized.contains("matches!(") && mentions_kind(&normalized) {
        if let Some(found) = banned_token_present(&normalized) {
            return Some(format!(
                "matches!-on-kind ({found}) in `{}`",
                code_line.trim()
            ));
        }
    }

    // Shape E: `[..banned..].contains(&kind)` / `.contains(&raw.kind)`.
    if normalized.contains(".contains(&") && mentions_kind(&normalized) {
        if let Some(found) = banned_token_present(&normalized) {
            return Some(format!(
                "contains-on-kind-set ({found}) in `{}`",
                code_line.trim()
            ));
        }
    }

    None
}

/// Find a banned kind literal (as a standalone integer token) or a banned kind
/// constant anywhere in a normalized (space-stripped) line. Used by the
/// `match`/`matches!`/`contains` shape heuristics.
fn banned_token_present(normalized: &str) -> Option<String> {
    for c in BANNED_GUARD_CONSTANTS {
        if normalized.contains(c) {
            return Some((*c).to_string());
        }
    }
    // Standalone integer token: the literal is bounded by a non-digit on both
    // sides so `1059` matches but `10591` / `30023` / `21059` do not.
    let bytes = normalized.as_bytes();
    for lit in BANNED_KIND_LITERALS {
        let mut from = 0;
        while let Some(rel) = normalized[from..].find(*lit) {
            let start = from + rel;
            let end = start + lit.len();
            let prev_ok = start == 0 || !bytes[start - 1].is_ascii_digit();
            let next_ok = end >= bytes.len() || !bytes[end].is_ascii_digit();
            // Also require the preceding char to not be alphabetic/underscore
            // (so `KIND_NIP14` or an identifier ending in the digits is not a
            // false hit) and the literal to be adjacent to a pattern operator
            // (`=>`, `|`, `,`, `(`, `&`) — the contexts a kind guard uses.
            let prev_char = if start == 0 {
                None
            } else {
                Some(bytes[start - 1])
            };
            let alpha_prefix = prev_char.is_some_and(|c| c.is_ascii_alphabetic() || c == b'_');
            if prev_ok && next_ok && !alpha_prefix {
                return Some((*lit).to_string());
            }
            from = end;
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

/// SHOULD-FIX #3 — the detector must also catch the `==`-evasion shapes:
/// `match kind { 1059 => }`, `matches!(kind, 1059 | 14)`,
/// `[1059, 14].contains(&kind)`, and the constant forms. One assertion per
/// evasion shape so a regression that drops one shape is caught.
#[test]
fn gate_detector_fires_on_evasion_shapes() {
    // `match` on a kind expression dispatching on a private literal.
    assert!(
        kind_policy_guard_violation("match kind { 1059 => fail_closed(), _ => route() }").is_some(),
        "must catch `match kind {{ 1059 => .. }}`"
    );
    // A match arm on its own line (match head on a previous line).
    assert!(
        kind_policy_guard_violation("            1059 => return Vec::new(),").is_some(),
        "must catch a standalone `1059 =>` match arm"
    );
    assert!(
        kind_policy_guard_violation("            1059 | 14 => fail_closed(),").is_some(),
        "must catch a multi-literal `1059 | 14 =>` arm"
    );
    // `matches!` macro on a kind expression.
    assert!(
        kind_policy_guard_violation("if matches!(kind, 1059 | 14) { fail_closed() }").is_some(),
        "must catch `matches!(kind, 1059 | 14)`"
    );
    // Slice/array `.contains(&kind)`.
    assert!(
        kind_policy_guard_violation("if [1059, 14].contains(&kind) { fail_closed() }").is_some(),
        "must catch `[1059, 14].contains(&kind)`"
    );
    // Constant-form evasions.
    assert!(
        kind_policy_guard_violation("match kind { KIND_GIFT_WRAP => fail_closed(), _ => {} }")
            .is_some(),
        "must catch a `match kind` arm on KIND_GIFT_WRAP"
    );
    assert!(
        kind_policy_guard_violation("matches!(raw.kind, KIND_CHAT_MESSAGE)").is_some(),
        "must catch `matches!(.., KIND_CHAT_MESSAGE)`"
    );

    // NO false positives on legitimate routing-surface shapes:
    assert!(
        kind_policy_guard_violation("PublishAction::PublishRaw { kind, target, .. } => {")
            .is_none(),
        "an enum-variant match arm that merely binds `kind` is not a policy guard"
    );
    assert!(
        kind_policy_guard_violation("created_at: 0,").is_none(),
        "a struct field `created_at: 0` is not a kind guard (no `kind` token)"
    );
    assert!(
        kind_policy_guard_violation("let unsigned = UnsignedEvent { kind, tags, content };")
            .is_none(),
        "constructing an event with a `kind` field is not a guard"
    );
    assert!(
        kind_policy_guard_violation("relay = %relay_url, kind, \"...\"").is_none(),
        "a tracing field list naming `kind` is not a guard"
    );
    // A non-policy kind literal (e.g. kind:30023 long-form) is NOT banned.
    assert!(
        kind_policy_guard_violation("match kind { 30023 => longform(), _ => {} }").is_none(),
        "kind:30023 is public-routable, not a policy-bearing reserved/private kind"
    );
}

/// Strip a trailing line comment so the gate scans code, not prose. A `//`
/// inside a string literal is rare on a guard line and would only ever cause a
/// false *negative* on the comment tail, never a false positive on code.
fn strip_comment(line: &str) -> &str {
    line.split("//").next().unwrap_or(line)
}
