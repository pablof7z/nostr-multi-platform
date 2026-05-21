//! Positive D10 fixture — must trigger at least one D10 finding.
//!
//! This file is NEVER compiled (Cargo only picks up files referenced from
//! a Cargo.toml `path = ...` entry). It exists solely as text for the
//! doctrine-lint smoke test to scan.
//!
//! Each function below opts into D10 with the `// D10: private-kind publish`
//! marker, then deliberately calls an Auto-routing publish seam — every
//! such line must be flagged.

pub fn send_gift_wrap_via_auto() {
    // D10: private-kind publish
    let signed = build_signed_kind_1059();
    // Auto-routing variant of the signed-publish call — D10 violation.
    kernel.publish_signed(&signed, &[]);
}

pub fn send_gift_wrap_with_explicit_auto_target() {
    // D10: private-kind publish
    let signed = build_signed_kind_1059();
    // The literal `PublishTarget::Auto` token — D10 violation.
    kernel.publish_signed_to(&signed, &[], PublishTarget::Auto);
}

pub fn send_gift_wrap_via_unsigned_auto() {
    // D10: private-kind publish
    let unsigned = build_kind_14_rumor();
    // Auto-variant of the unsigned-publish actor command — D10 violation.
    let _ = publish_unsigned_event(identity, kernel, unsigned, ps);
}
