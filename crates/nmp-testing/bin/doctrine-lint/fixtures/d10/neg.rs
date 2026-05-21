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
