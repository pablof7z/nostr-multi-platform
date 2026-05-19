//! `RelayAuthState` — re-exported from the `nmp-nip42-types` substrate.
//!
//! **T77:** this module used to define its own `RelayAuthState` enum and a
//! `relay_auth_state_to_subs` translation function whose sole purpose was
//! to copy between this enum and the variant-identical placeholder in
//! `nmp_core::subs::trigger`. Both crates now share the single
//! `nmp_nip42_types::RelayAuthState`, so the placeholder and the
//! translation function are gone — `nmp_core::subs::RelayAuthState` and
//! this type are literally the same type, no conversion needed.

pub use nmp_nip42_types::RelayAuthState;
