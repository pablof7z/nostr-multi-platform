//! Positive D11 fixture — must trigger at least one D11 finding.
//!
//! This file is NEVER compiled (Cargo only picks up files referenced from
//! a Cargo.toml `path = ...` entry). It exists solely as text for the
//! doctrine-lint smoke test to scan. PR-F (one door per capability)
//! deleted the bespoke event-producing FFI; this fixture pretends a host
//! has re-introduced it under a new symbol name, which D11 must catch.

// Same-line opener variant — the common FFI shape.
pub extern "C" fn nmp_app_publish_legacy_signed(_app: *mut NmpApp, _event_json: *const c_char) {
    // D11 fires here: a new `nmp_app_*` FFI body sending
    // `ActorCommand::PublishSignedEvent` re-opens the door PR-F deleted.
    let _ = ActorCommand::PublishSignedEvent {
        raw: r,
        relays: v,
        correlation_id: c,
    };
}

// Wrapped multi-line signature — same offence, different opener shape.
pub extern "C" fn nmp_app_smuggle_unsigned(
    _app: *mut NmpApp,
    _unsigned_json: *const c_char,
) {
    // D11 fires here: `ActorCommand::PublishUnsignedEvent(_)` inside a new
    // `nmp_app_*` body is the deleted unsigned door.
    let _ = ActorCommand::PublishUnsignedEvent(u);
}
