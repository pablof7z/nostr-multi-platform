//! Negative D11 fixture — must produce ZERO findings of any rule.
//!
//! Exercises D11 exemptions:
//!   1. A non-FFI helper builds a banned variant — D11 only fires inside NMP
//!      app C-entry bodies, so this is exempt.
//!   2. An `extern "C" fn` whose verb is NOT NMP-app-prefixed is out
//!      of scope (D11 is the door for the `nmp-core` FFI surface).
//!   3. A trailing-comment `// doctrine-allow: D11` opts out a specific
//!      body line (the standard doctrine escape hatch).
//!
//! Care: the fixture text is ALSO scanned for D6/D7/D8 — no `.unwrap()` /
//! `todo!()` / hot-path allocations / sleeps may appear, or the negative
//! assertion (zero findings of any rule) breaks.

// (1) Non-FFI helper — D11 must not fire. This is exactly the
// `kernel::action_registry` shape (the GOOD path: dispatch_action's
// executor builds a `PublishSignedEvent`).
pub fn route_publish_action() {
    let _ = ActorCommand::Publish(PublishCommand::SignedEvent {
        raw: r,
        relays: v,
        correlation_id: c,
    });
}

// (2) Different FFI prefix — out of D11 scope.
pub extern "C" fn nmp_signer_broker_internal_hypothetical(_app: *mut SomeType) {
    let _ = ActorCommand::Publish(PublishCommand::SignedEvent {
        raw: r,
        relays: v,
        correlation_id: c,
    });
}

// (3) Per-line escape hatch — explicit opt-out on the offending body line.
pub extern "C" fn nmp_app_exempt_via_allow(_app: *mut NmpApp) {
    let _ = ActorCommand::Publish(PublishCommand::SignedEvent { raw: r, relays: v, correlation_id: c }); // doctrine-allow: D11 - fixture exemption proof
}
