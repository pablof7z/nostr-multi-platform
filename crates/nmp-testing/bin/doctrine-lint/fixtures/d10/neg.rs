//! Negative D10 fixture — must produce zero D10 findings.
//!
//! This file demonstrates every shape D10 must NOT fire on:
//!
//! - An unmarked function may freely use Auto-routing — D10 fires only
//!   inside marked private-kind publishers.
//! - A marked function using Explicit pinning is clean — that's the
//!   correct shape for a kind:1059 publisher.
//! - The `// doctrine-allow: D10 — <reason>` escape hatch suppresses a
//!   finding on a single legitimately-Auto private-kind path.
//! - Comments mentioning the banned tokens are inert.

// An ordinary, unmarked publisher — Auto-routing is the right answer for
// kind:1 / 3 / 7 etc., and D10 must not interfere.
pub fn publish_normal_note() {
    let signed = build_signed_kind_1();
    kernel.publish_signed(&signed, &[]);
}

// A marked private-kind publisher that does the right thing: derives an
// explicit pin from the recipient's kind:10050 DM-inbox cache and routes
// via the Explicit-target sibling. Zero D10 findings expected.
pub fn send_gift_wrap_correctly() {
    // D10: private-kind publish
    let signed = build_signed_kind_1059();
    let pin = kernel.recipient_dm_relays(receiver_hex).unwrap_or_default();
    // Explicit-pin variant — the underscore-`to` form is NOT flagged.
    kernel.publish_signed_to(&signed, &[], PublishTarget::Explicit { relays: pin });
}

// A marked publisher that uses the Explicit-pin variant with correlation —
// again Explicit, again clean.
pub fn send_gift_wrap_with_correlation() {
    // D10: private-kind publish
    let signed = build_signed_kind_1059();
    let pin = recipient_dm_relays(receiver_hex);
    kernel.publish_signed_to_with_correlation(&signed, &[], PublishTarget::Explicit { relays: pin }, None);
}

// The escape hatch — a documented Auto-fallback in a legitimately-marked
// private-kind publisher must be suppressed by the per-line annotation.
pub fn send_gift_wrap_with_documented_escape() {
    // D10: private-kind publish
    let signed = build_signed_kind_1059();
    kernel.publish_signed(&signed, &[]); // doctrine-allow: D10 — test fixture covering the documented escape hatch
}

// PR-K3 counterpart to `pos.rs::dispatch_kind1059_via_empty_relays`. The
// production dispatch arm in `actor::dispatch::PublishSignedEvent` is
// INTENTIONALLY unmarked. The arm is kind-agnostic — it forwards
// `ActorCommand::PublishSignedEvent` for kind:1, kind:3, kind:30023 etc.
// where empty-relays → Auto is the CORRECT behaviour. Marking the arm would
// fire D10 on every non-1059 dispatch.
//
// The structural defense against a kind:1059 leak through the dispatch arm
// lives in the runtime guard at the top of
// `commands::publish::publish_signed_event` (PR-K3): it refuses any
// kind:1059 publish whose `relays` slice is empty, regardless of which
// caller (dispatch arm, NIP-17 send path, Marmot bridge, future paths)
// reached the function. The lint discipline stays where the kind context
// is unambiguous; the runtime guard handles the kind-agnostic seam.
pub fn dispatch_arm_intentionally_unmarked() {
    // No D10 marker on purpose (referring to the rule by id only; quoting
    // the literal marker substring here would inadvertently open the
    // marked scope per the known limitation in `rules/d10.rs`). The
    // empty-relays call below MUST stay silent — D10 is dormant outside
    // marked fns by design.
    let raw = build_raw_signed_event();
    let relays: Vec<String> = Vec::new();
    let _ = publish_signed_event(kernel, raw, &relays, None);
}
