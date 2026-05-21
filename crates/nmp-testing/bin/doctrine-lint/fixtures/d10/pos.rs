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

// PR-K3 positive fixture — proves the D10 banned-list (`publish_signed_event(`)
// catches the empty-relays Auto-route path inside a marked dispatcher.
//
// This is the synthetic shape `actor::dispatch::PublishSignedEvent` would have
// if it ever opted into D10. The production dispatch arm is INTENTIONALLY
// unmarked because it is kind-agnostic (it forwards `ActorCommand::PublishSignedEvent`
// for every kind, not just kind:1059); the structural defense for the
// dispatch arm lives in the runtime guard at the top of
// `commands::publish::publish_signed_event` (PR-K3). The fixture below
// exists to prove that IF an author ever does add the marker — for whatever
// reason — the lint catches the leak. See `neg.rs::dispatch_arm_intentionally_unmarked`
// for the counterpart documenting the production decision.
pub fn dispatch_kind1059_via_empty_relays() {
    // D10: private-kind publish
    // The empty-relays path inside a marked dispatcher: even though
    // `publish_signed_event` itself has a kind:1059 guard, the lint must
    // ALSO catch this shape so a future marker'd dispatcher cannot regress.
    let raw = build_signed_kind_1059_raw();
    let relays: Vec<String> = Vec::new();
    let _ = publish_signed_event(kernel, raw, &relays, None);
}
