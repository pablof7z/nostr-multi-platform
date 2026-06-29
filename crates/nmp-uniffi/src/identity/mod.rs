//! Identity, account, relay, and signer UniFFI surface — M14-C2.
//!
//! Migrates the C-ABI symbols from `nmp-ffi/src/{identity,signer_broker,external_signer}.rs`
//! to typed `#[uniffi::export] impl NmpApp` methods. This is **additive** — the C-ABI
//! symbols are NOT deleted here.
//!
//! ## Module layout
//!
//! | Module       | UniFFI methods                                           | C-ABI counterpart          |
//! |--------------|----------------------------------------------------------|----------------------------|
//! | `account`    | `signin_nsec`, `register_agent_nsec`, `create_new_account`, `switch_active`, `remove_account`, `signin_bunker` | `nmp-ffi/src/identity.rs`  |
//! | `relay`      | `add_relay`, `remove_relay`                              | `nmp-ffi/src/identity.rs`  |
//! | `broker`     | `init_signer_broker`, `cancel_bunker_handshake`, `nostrconnect_uri` | `nmp-ffi/src/signer_broker.rs` |
//! | `external`   | `init_external_signer`, `signin_nip55`, `deliver_external_signer_response` | `nmp-ffi/src/external_signer.rs` |
//!
//! ## Design notes
//!
//! * Each module adds a `#[uniffi::export] impl NmpApp` block. UniFFI collects
//!   all blocks for the same object across modules.
//! * Every method calls `self.inner.<method>(...)` — the same underlying
//!   `nmp_native_runtime::NmpApp` method the C-ABI wrapper calls. No logic is
//!   duplicated.
//! * `RelayConfigEntry` is a typed record for the relay pairs used in
//!   `create_new_account` instead of the C-ABI's JSON-decoded `Vec<(String, String)>`.
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
