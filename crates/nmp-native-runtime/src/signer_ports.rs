//! ADR-0072 §D3 — per-app signer-port accessors on [`NmpApp`].
//!
//! Split out of `lib.rs` (file-size discipline) as a cohesive `impl NmpApp`
//! block. These methods own the per-app bunker / NIP-55 **hook-slot** install
//! + invoke surface and the per-app NIP-46 **runtime** / NIP-55 **driver**
//! handle accessors that replaced the deleted `GLOBAL_BROKER` / `GLOBAL_DRIVER`
//! process-globals (and the two `nmp-core` hook statics). Every handle here
//! lives on the `NmpApp` and dies with it — no global aliasing across
//! `nmp_app_free`.

#[cfg(feature = "external-signer")]
use std::sync::Arc;

use super::NmpApp;

impl NmpApp {
    /// ADR-0072 §D3 — install the per-app bunker-URI hook (the runtime's
    /// `start_bunker_connect` / `restore_session` dispatch). Called by
    /// `nmp_signer_broker_init`. Replaces the deleted `register_bunker_hook`
    /// process-global write.
    // Live only under `signer-broker` (production) or `test`/`test-support`
    // (via `signer_ports_test_support`). cfg-gated to avoid a dead_code warning
    // in plain `native` builds.
    #[cfg(any(feature = "signer-broker", test, feature = "test-support"))]
    pub(crate) fn install_bunker_hook(&self, hook: nmp_core::BunkerHookFn) {
        nmp_core::install_bunker_hook(&self.composition.bunker_hook, hook);
    }

    /// ADR-0072 §D3 — install the per-app NIP-55 external-signer restore hook.
    /// Called by `nmp_external_signer_init`. Replaces the deleted
    /// `register_external_signer_hook` process-global write.
    // Live only under `external-signer` (production) or `test`/`test-support`
    // (via `signer_ports_test_support`). cfg-gated to avoid a dead_code warning
    // in plain `native` builds.
    #[cfg(any(feature = "external-signer", test, feature = "test-support"))]
    pub(crate) fn install_external_signer_hook(&self, hook: nmp_core::ExternalSignerHookFn) {
        nmp_core::install_external_signer_hook(&self.capability_ports.external_signer_hook, hook);
    }

    /// ADR-0072 §D3 — test-support: invoke this app's bunker connect hook
    /// through its per-app slot (the rung 5.3 isolation oracle). Mirrors the
    /// actor's `start_bunker_handshake` read without the wire.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn invoke_bunker_connect_hook_for_test(&self, uri: &str) -> bool {
        nmp_core::bunker_hook::invoke_bunker_connect_hook_for_test(
            &self.composition.bunker_hook,
            uri,
        )
    }

    /// ADR-0072 §D3 — test-support: invoke this app's NIP-55 restore hook
    /// through its per-app slot (the rung 5.3 isolation oracle).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn invoke_external_signer_restore_hook_for_test(&self, payload_json: &str) -> bool {
        nmp_core::external_signer_hook::invoke_external_signer_restore_hook_for_test(
            &self.capability_ports.external_signer_hook,
            payload_json,
        )
    }

    /// ADR-0072 §D3 — per-app NIP-55 driver handle accessor (replacing
    /// `GLOBAL_DRIVER`). Idempotent first-writer-wins, mirroring the broker.
    #[cfg(feature = "external-signer")]
    pub(crate) fn external_signer_driver_get_or_init(
        &self,
        init: impl FnOnce() -> Arc<crate::external_signer::Nip55Driver>,
    ) -> Arc<crate::external_signer::Nip55Driver> {
        let mut guard = self
            .external_signer_driver
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(existing) = guard.as_ref() {
            return Arc::clone(existing);
        }
        let driver = init();
        *guard = Some(Arc::clone(&driver));
        driver
    }

    /// ADR-0072 §D3 — read the per-app NIP-55 driver handle (signin / deliver
    /// symbols). `None` before `nmp_external_signer_init`.
    #[cfg(feature = "external-signer")]
    pub(crate) fn external_signer_driver(
        &self,
    ) -> Option<Arc<crate::external_signer::Nip55Driver>> {
        self.external_signer_driver
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map(Arc::clone)
    }
}
