//! The single composition-root entry point for `nmp-mint-discovery`: wires
//! the identity-reactive read interests ([`MintDiscoveryRuntime`]) and this
//! crate's own typed `"mint_discovery"` snapshot projection.
//!
//! Mirrors the canonical `register(app, Config) -> Result<Handles,
//! RegistrationError>` shape every reusable NMP protocol crate exposes (see
//! `nmp_wot::register`, `nmp_nip50::register`, …) — a production app
//! composition root calls this alongside its other named protocol
//! installers:
//!
//! ```ignore
//! let _mint_discovery = nmp_mint_discovery::register(
//!     app,
//!     nmp_mint_discovery::Config::default(),
//! ).expect("nmp-mint-discovery registration must not collide");
//! ```
//!
//! This crate has no dependency on `nmp-wallet` and vice versa — an app can
//! compose mint discovery on its own, or alongside a wallet, at its own
//! composition root.

use std::sync::Arc;

use nmp_core::substrate::{
    HostCapabilities, IdentityChangeRegistrar, ObservedProjectionRegistrar, RegistrationError,
    SnapshotProjectionRegistrar,
};

use crate::discovery::DiscoveryPolicy;
use crate::runtime::MintDiscoveryRuntime;

/// Composition config for [`register`].
#[derive(Clone, Debug, Default)]
pub struct Config {
    /// The discovery policy (required NUTs, minimum recommender score,
    /// optional cold-start `fallback_root`, result cap). `Default::default()`
    /// reproduces the nutzap-required-NUTs, no-fallback-root policy this
    /// crate shipped with when it lived inside `nmp-wallet`.
    pub policy: DiscoveryPolicy,
}

/// Handles returned by [`register`].
pub struct Handles {
    /// The installed mint-discovery runtime. Owns the identity-reactive read
    /// interests for kind:38172 announcements + kind:38000 recommendations
    /// and the viewer's follow/mute graph, and produces the
    /// web-of-trust-scoped, capability-fail-closed discovered-mints
    /// projection via [`MintDiscoveryRuntime::snapshot`]. Held so a
    /// composition root can query discovered mints directly in Rust, in
    /// addition to the typed `"mint_discovery"` sidecar this function also
    /// registers.
    pub runtime: Arc<MintDiscoveryRuntime>,
}

/// Register the mint-discovery composition stack on `app`: the identity-
/// reactive [`MintDiscoveryRuntime`] plus this crate's own typed
/// `"mint_discovery"` snapshot projection (see `projection_wire.rs`).
pub fn register(
    app: &mut (impl ObservedProjectionRegistrar
              + IdentityChangeRegistrar
              + SnapshotProjectionRegistrar
              + HostCapabilities),
    config: Config,
) -> Result<Handles, RegistrationError> {
    let active_pubkey = app.active_pubkey();
    let runtime = Arc::new(MintDiscoveryRuntime::with_policy(
        active_pubkey,
        app,
        config.policy,
    ));

    // Typed `"mint_discovery"` snapshot projection: a read-only, non-blocking
    // producer (D8) that holds its own `Arc<MintDiscoveryRuntime>` clone (no
    // process-global). Never calls the `audit` feature's `enrich_with_audit`
    // here — that performs real HTTP and must run off this emit path (see
    // `audit.rs`'s module docs).
    let projection_runtime = Arc::clone(&runtime);
    app.register_typed_snapshot_projection(
        nmp_ownership::DeclaredProjectionKey::framework(
            crate::projection_wire::PROJECTION_KEY,
            "projection.mint_discovery",
        ),
        move || Some(mint_discovery_typed_projection(&projection_runtime)),
    );

    Ok(Handles { runtime })
}

/// Build the typed `"mint_discovery"` sidecar entry from the live runtime's
/// snapshot. Extracted from the `register_typed_snapshot_projection` closure
/// so the registration's schema identity (`key` / `schema_id` /
/// `file_identifier` / version) and the encode are unit-testable without
/// spinning the actor.
///
/// Always emits a row (even when empty — no account active yet, or no mints
/// discovered): an omitted key retains the last decoded value under
/// incremental apply (ADR-0070), so a well-formed empty projection keeps the
/// host cache authoritative rather than leaving a stale value cached.
#[must_use]
pub fn mint_discovery_typed_projection(
    runtime: &MintDiscoveryRuntime,
) -> nmp_core::TypedProjectionData {
    let projection = runtime.snapshot();
    nmp_core::TypedProjectionData {
        key: crate::projection_wire::PROJECTION_KEY.to_string(),
        schema_id: crate::projection_wire::SCHEMA_ID.to_string(),
        schema_version: crate::projection_wire::SCHEMA_VERSION,
        file_identifier: String::from_utf8_lossy(crate::projection_wire::FILE_IDENTIFIER)
            .into_owned(),
        payload: crate::projection_wire::encode_mint_discovery_projection(&projection),
        ..Default::default()
    }
}

#[cfg(test)]
#[path = "register_tests.rs"]
mod tests;
