//! `nmp-nip11` — NIP-11 relay information documents as an NMP protocol crate.
//!
//! NMP owns the full NIP-11 lifecycle so consumer apps get relay names, icons,
//! and capability metadata with ZERO work of their own — no HTTP, no JSON, no
//! awareness of what NIP-11 is. See ADR-0072.
//!
//! Two trigger paths, one parsed type ([`nmp_core::substrate::RelayInfoDoc`]),
//! one diagnostics surface:
//!
//! 1. **Automatic, on connect.** [`register`] installs a [`Nip11FetchHook`] on
//!    the app's [`nmp_core::substrate::RelayConnectedHookSlot`]. The actor fans
//!    it on every `PoolEvent::Opened`; subject to a per-URL TTL the hook spawns
//!    an off-thread [`fetch_relay_info_blocking`] and posts the result back via
//!    `ActorCommand::SetRelayInfo`. The document then appears on the relay's
//!    `relay_diagnostics` row — apps need no per-relay probe call for pool
//!    relays.
//!
//! 2. **On-demand probe.** [`probe_relay_info`] fetches a relay that is NOT yet
//!    in the pool (the "add relay" preview flow), returning the same
//!    [`RelayInfoDoc`]. It is blocking; FFI wraps it on a worker thread.
//!
//! `nmp-core` learns no NIP-11 noun and imports no HTTP crate (D0): the
//! [`RelayInfoDoc`] it carries is substrate-generic relay transport metadata;
//! the HTTP fetch + parse live here.

pub mod fetch;
pub mod hook;
pub mod parse;
pub mod url;

pub use fetch::fetch_relay_info_blocking;
pub use hook::{Nip11FetchHook, NIP11_TTL};
pub use nmp_core::substrate::RelayInfoDoc;
pub use parse::parse_relay_info;
pub use url::http_url_for_relay;

#[derive(Clone, Debug, Default)]
pub struct Config {}

#[derive(Clone, Debug, Default)]
pub struct Handles {}

/// Install the automatic NIP-11 fetch hook on `app`. After this call, every
/// relay the pool connects to has its information document fetched and surfaced
/// through the `relay_diagnostics` projection automatically — no further app
/// involvement (ADR-0072 path 1).
///
/// `app` is any [`nmp_core::substrate::RelayConnectedHookRegistrar`] —
/// `nmp_native_runtime::NmpApp` in production, wired up by each app's explicit
/// composition (the native `NmpAppBuilder` / `nmp-uniffi` binding surface;
/// there is no `nmp-ffi` crate). This crate stays decoupled from the binding
/// surface by depending on the narrow relay-connected-hook registration
/// trait, not the concrete app and not the broad `AppHost` (D6 capability
/// honesty: this crate only reacts to relay connects).
pub fn register(
    app: &impl nmp_core::substrate::RelayConnectedHookRegistrar,
    _config: Config,
) -> Result<Handles, nmp_core::substrate::RegistrationError> {
    app.add_relay_connected_hook(std::sync::Arc::new(Nip11FetchHook::new()));
    Ok(Handles {})
}

/// On-demand probe of an arbitrary relay URL that may not be in the pool
/// (ADR-0072 path 3) — the "add relay" preview flow. BLOCKING: callers run it
/// on a worker thread (the FFI shim does). Returns the same [`RelayInfoDoc`]
/// the automatic path produces, or an error string the caller surfaces as
/// "couldn't reach relay".
pub fn probe_relay_info(relay_url: &str) -> Result<RelayInfoDoc, String> {
    fetch_relay_info_blocking(relay_url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::{
        fan_relay_connected, install_relay_connected_hook, new_relay_connected_hook_slot,
    };
    use nmp_core::{ActorMail, CommandSender};
    use std::sync::Arc;

    /// End-to-end of the hook seam without a real `AppHost`: install the
    /// `Nip11FetchHook` on a bare slot, fan a connect, and assert the hook
    /// spawns a fetch (it cannot post a real `SetRelayInfo` without network,
    /// but the unmappable URL means it returns without sending — proving the
    /// fan + spawn path runs without panicking).
    #[test]
    fn installed_hook_runs_on_fan_without_panicking() {
        let slot = new_relay_connected_hook_slot();
        install_relay_connected_hook(&slot, Arc::new(Nip11FetchHook::new()));
        let (tx, _rx) = std::sync::mpsc::channel::<ActorMail>();
        fan_relay_connected(&slot, "not-a-relay", false, &CommandSender::new(tx));
        // Give the spawned worker a moment to run and exit on the unmappable
        // URL; nothing should be posted back.
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    #[test]
    fn probe_rejects_unmappable_url_offline() {
        assert!(probe_relay_info("not-a-relay").is_err());
    }
}

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
