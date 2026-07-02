//! Positive D11 fixture — must trigger at least one D11 finding.
//!
//! This file is NEVER compiled (Cargo only picks up files referenced from
//! a Cargo.toml `path = ...` entry). It exists solely as text for the
//! doctrine-lint smoke test to scan. ADR-0071 made `NmpApp::dispatch_action`
//! the one UniFFI publish doorway; this fixture pretends a host has
//! re-introduced a bespoke publish-producing `#[uniffi::export]` method,
//! which D11 must catch.

// A bespoke `#[uniffi::export]`-attributed impl block whose method
// constructs a publish command directly, bypassing `dispatch_action`.
#[uniffi::export]
impl LegacyDoor {
    pub fn publish_legacy_signed(&self, event_json: String) {
        // D11 fires here: a new `#[uniffi::export]` method sending
        // `ActorCommand::PublishSignedEvent` re-opens the door ADR-0071 closed.
        let _ = ActorCommand::Publish(PublishCommand::SignedEvent {
            raw: r,
            relays: v,
            correlation_id: c,
        });
    }
}

// A bespoke `#[uniffi::export]`-attributed free function — same offence.
#[uniffi::export]
pub fn smuggle_unsigned(unsigned_json: String) {
    // D11 fires here: `ActorCommand::PublishUnsignedEvent(_)` inside a new
    // `#[uniffi::export]` body is the deleted unsigned door.
    let _ = ActorCommand::PublishUnsignedEvent(u);
}

// Bare C-ABI tombstone: this shape must never reappear at all, regardless
// of whether it constructs a banned variant.
pub extern "C" fn nmp_app_publish_signed_event(_app: *mut NmpApp) {}
