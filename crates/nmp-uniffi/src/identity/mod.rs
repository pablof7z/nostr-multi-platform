//! Identity, account, relay, and signer UniFFI surface.
//!
//! `nmp-uniffi` is the sole native binding surface for identity, account,
//! relay, and signer operations (M14 complete; the legacy `nmp-ffi` C-ABI
//! crate has been deleted). Each sub-module adds a
//! `#[uniffi::export] impl NmpApp` block exposing typed methods.
//!
//! ## Module layout
//!
//! | Module       | UniFFI methods                                           |
//! |--------------|----------------------------------------------------------|
//! | `account`    | `signin_nsec`, `register_agent_nsec`, `create_new_account`, `switch_active`, `remove_account`, `signin_bunker` |
//! | `relay`      | `add_relay`, `remove_relay`                              |
//! | `broker`     | `init_signer_broker`, `cancel_bunker_handshake`, `nostrconnect_uri` |
//! | `external`   | `init_external_signer`, `signin_nip55`, `deliver_external_signer_response` |
//!
//! ## Design notes
//!
//! * Each module adds a `#[uniffi::export] impl NmpApp` block. UniFFI collects
//!   all blocks for the same object across modules.
//! * Every method calls `self.inner.<method>(...)` on the underlying
//!   `nmp_native_runtime::NmpApp` — no logic is duplicated here.
//! * `RelayConfigEntry` is a typed record for the relay pairs used in
//!   `create_new_account` (rather than a JSON-decoded `Vec<(String, String)>`).
//! * `broker` and `external` are behind their respective feature flags, which
//!   are both included in the `native` default feature so generated bindings
//!   always cover the full identity surface.

pub mod account;
pub mod relay;

#[cfg(feature = "signer-broker")]
pub mod broker;

#[cfg(feature = "external-signer")]
pub mod external;

// ── Shared types ──────────────────────────────────────────────────────────────

/// A relay URL + role pair used when creating a new account.
///
/// Mirrors the `Vec<(String, String)>` shape that the C-ABI
/// `nmp_app_create_new_account` parses from a JSON string — but typed.
/// `role` is a string like `"read"`, `"write"`, or `"both"`.
#[derive(uniffi::Record, Debug, Clone)]
pub struct RelayConfigEntry {
    pub url: String,
    pub role: String,
}
