//! T66a actor command handlers — identity / publish / relay-edit.
//!
//! ## D0 boundary
//!
//! The actor lives in `nmp-core`. `nmp-signers` depends on `nmp-core`, so
//! `nmp-core` CANNOT import `nmp-signers` (would be a dependency cycle).
//! `AccountManager` / `LocalKeySigner` / `Nip46Signer` therefore cannot be
//! used here. Instead the actor keeps a local `IdentityRuntime` of bare
//! `nostr::Keys` and adapts the active key to the kernel's existing
//! `AuthSignerFn` seam (`Kernel::bind_auth_signer`). This is the same
//! primitive the existing `nmp_app_inject_signed_events` FFI already uses
//! (`ffi.rs` — `nostr::Keys` + `EventBuilder::sign_with_keys`). Full
//! `AccountManager` integration is M14 (UniFFI), when the FFI surface can
//! move to a crate that may depend on both `nmp-core` and `nmp-signers`.
//!
//! ## NIP-46
//!
//! `bunker://` URIs are parsed + shape-validated here, but the NIP-46
//! transport is NOT wired (same D0 reason — `Nip46Signer` is in
//! `nmp-signers`). `sign_in_bunker` surfaces a `last_error_toast` directing
//! the user to nsec for this build. The build doc (§11) explicitly
//! authorizes shipping nsec-only multi-account.

mod identity;
mod publish;
mod relays;
#[cfg(test)]
mod tests;

pub(super) use identity::{
    create_account, remove_account, sign_in_bunker, sign_in_nsec, switch_active, IdentityRuntime,
};
pub(super) use publish::{follow, open_timeline, publish_note, react};
pub(super) use relays::{add_relay, remove_relay};
