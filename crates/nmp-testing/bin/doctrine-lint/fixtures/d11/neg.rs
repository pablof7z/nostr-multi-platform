//! Negative D11 fixture — must produce ZERO findings of any rule.
//!
//! Exercises every D11 exemption:
//!   1. Whitelisted symbols (`nmp_app_retry_publish`, `nmp_app_cancel_publish`)
//!      construct banned variants — the whitelist must suppress them.
//!   2. A non-FFI helper builds a banned variant — D11 only fires inside
//!      `extern "C" fn nmp_app_*` bodies, so this is exempt.
//!   3. An `extern "C" fn` whose verb is NOT `nmp_app_*`-prefixed is out
//!      of scope (D11 is the door for the `nmp-core` FFI surface).
//!   4. A trailing-comment `// doctrine-allow: D11` opts out a specific
//!      body line (the standard doctrine escape hatch).
//!
//! Care: the fixture text is ALSO scanned for D6/D7/D8 — no `.unwrap()` /
//! `todo!()` / hot-path allocations / sleeps may appear, or the negative
//! assertion (zero findings of any rule) breaks.

// (1) Whitelisted symbol — construction must be ignored.
pub extern "C" fn nmp_app_retry_publish(_app: *mut NmpApp, _handle: *const c_char) {
    // The real body uses `RetryPublish`; the fixture jams the banned
    // variant here just to prove the whitelist works.
    let _ = ActorCommand::PublishSignedEvent { raw: r, relays: v, correlation_id: c };
}

// (1) Wrapped whitelisted signature — exemption flows through the wrapped
// opener path too.
pub extern "C" fn nmp_app_cancel_publish(
    _app: *mut NmpApp,
    _handle: *const c_char,
) {
    let _ = ActorCommand::PublishUnsignedEvent(u);
}

// (2) Non-FFI helper — D11 must not fire. This is exactly the
// `kernel::action_registry` shape (the GOOD path: dispatch_action's
// executor builds a `PublishSignedEvent`).
pub fn route_publish_action() {
    let _ = ActorCommand::PublishSignedEvent { raw: r, relays: v, correlation_id: c };
}

// (3) Different FFI prefix — out of D11 scope (the rule is the door for
// the `nmp_app_*` FFI surface only).
pub extern "C" fn nmp_signer_broker_internal_hypothetical(_app: *mut SomeType) {
    let _ = ActorCommand::PublishSignedEvent { raw: r, relays: v, correlation_id: c };
}

// (4) Per-line escape hatch — explicit opt-out on the offending body line.
pub extern "C" fn nmp_app_exempt_via_allow(_app: *mut NmpApp) {
    let _ = ActorCommand::PublishSignedEvent { raw: r, relays: v, correlation_id: c }; // doctrine-allow: D11 — fixture exemption proof
}
