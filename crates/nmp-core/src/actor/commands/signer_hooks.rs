//! ADR-0052 §D3 — per-app signer-hook accessors on [`IdentityRuntime`].
//!
//! Split out of `identity.rs` (file-size discipline) as a cohesive
//! `impl IdentityRuntime` block. These methods bind / install / invoke the
//! per-app bunker (`BunkerHookSlot`) and NIP-55 external-signer
//! (`ExternalSignerHookSlot`) hook slots that replaced the deleted
//! `bunker_hook::HOOK` / `external_signer_hook::HOOK` process-global statics.
//! The slots themselves are fields on `IdentityRuntime` (declared in
//! `identity.rs`); the actor binds them via [`IdentityRuntime::set_signer_hook_slots`]
//! and the bunker / NIP-55 restore command handlers invoke them.

use super::identity::IdentityRuntime;

impl IdentityRuntime {
    /// ADR-0052 §D3 — bind this runtime's per-app signer hook slots to the
    /// `Arc` clones the `NmpApp` holds, so the FFI composition root
    /// (`nmp_signer_broker_init` / `nmp_external_signer_init`) installs hooks
    /// into the SAME slots `start_bunker_handshake` / `restore_nip55_session`
    /// read. Called once by the actor at construction, after `new`. Replaces
    /// the deleted `register_bunker_hook` / `register_external_signer_hook`
    /// process-global statics.
    pub(crate) fn set_signer_hook_slots(
        &mut self,
        bunker_hook: crate::bunker_hook::BunkerHookSlot,
        external_signer_hook: crate::external_signer_hook::ExternalSignerHookSlot,
    ) {
        self.set_bunker_hook_slot(bunker_hook);
        self.set_external_signer_hook_slot(external_signer_hook);
    }

    /// Test-only: install a bunker hook directly into this runtime's per-app
    /// slot, so command-path unit tests can exercise the happy / no-hook
    /// branches deterministically (no process-global state to leak between
    /// tests, unlike the deleted `register_bunker_hook`).
    #[cfg(test)]
    pub(crate) fn install_bunker_hook_for_test(&self, hook: crate::bunker_hook::BunkerHookFn) {
        crate::bunker_hook::install_bunker_hook(self.bunker_hook_slot(), hook);
    }

    /// Test-only: install a NIP-55 restore hook directly into this runtime's
    /// per-app slot.
    #[cfg(test)]
    pub(crate) fn install_external_signer_hook_for_test(
        &self,
        hook: crate::external_signer_hook::ExternalSignerHookFn,
    ) {
        crate::external_signer_hook::install_external_signer_hook(
            self.external_signer_hook_slot(),
            hook,
        );
    }

    /// Invoke this app's installed bunker connect hook. `false` if no broker
    /// is installed (caller surfaces a toast).
    pub(crate) fn invoke_bunker_connect_hook(&self, uri: &str) -> bool {
        crate::bunker_hook::invoke_bunker_connect_hook(self.bunker_hook_slot(), uri)
    }

    /// Invoke this app's installed bunker restore hook. `false` if no broker.
    pub(crate) fn invoke_bunker_restore_hook(&self, payload_json: &str) -> bool {
        crate::bunker_hook::invoke_bunker_restore_hook(self.bunker_hook_slot(), payload_json)
    }

    /// Invoke this app's installed NIP-55 restore hook. `false` if no driver.
    pub(crate) fn invoke_external_signer_restore_hook(&self, payload_json: &str) -> bool {
        crate::external_signer_hook::invoke_external_signer_restore_hook(
            self.external_signer_hook_slot(),
            payload_json,
        )
    }
}
