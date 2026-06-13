//! Wallet (NIP-47) wiring step for [`NmpAppBuilder`](super::NmpAppBuilder).
//!
//! Extracted as a cohesive sibling submodule of `builder` so the
//! composition-root file stays under the 500-LOC hard ceiling
//! (AGENTS.md file-size rule). As a child module it retains access to the
//! parent's private `app` handle, so the public builder API is unchanged.

use super::{NmpAppBuilder, Unstarted};

// ── Wallet (NIP-47) wiring step (Unstarted) ─────────────────────────────────

impl NmpAppBuilder<Unstarted> {
    /// Wire the NIP-47 Nostr Wallet Connect stack as a typed builder step.
    ///
    /// # V-95 / issue #619 — install-before-dispatch made type-enforceable
    ///
    /// The wallet runtime must be installed before any `nmp.wallet.*` action
    /// dispatches: `WalletConnectModule::execute` /
    /// `WalletDisconnectModule::execute` / `WalletPayInvoiceModule::execute`
    /// all read the process-wide runtime handle via `active_wallet_runtime`,
    /// and return a runtime error if it was never installed. Previously the
    /// install lived in an app crate (`nmp-app-chirp::wallet_runtime`) with no
    /// compile-time ordering guarantee.
    ///
    /// Folding the install into the builder makes it a *config-phase* step:
    /// because `start()` consumes the builder by move, a Rust caller cannot
    /// reach `start()` without `.with_wallet()` having already installed the
    /// runtime. The ordering contract is now expressed in the type system, not
    /// in prose.
    ///
    /// The durable [`FsPaymentStore`](nmp_nip47::FsPaymentStore) is installed
    /// automatically when a persistent storage path is configured — i.e. when
    /// `.storage_path(p)` was called before this step (or
    /// `nmp_app_set_storage_path` was driven through the C-ABI). With
    /// `.in_memory()` (no path), the runtime tracks payments in memory only,
    /// exactly as before.
    ///
    /// Idempotent at the process level: the underlying
    /// `install_wallet_runtime` is a one-shot `OnceLock`; a second
    /// `.with_wallet()` (e.g. across two builders in one test process) is a
    /// silent no-op for the install (the first handle wins) while still
    /// registering this builder's action modules + projections.
    #[must_use]
    pub fn with_wallet(mut self) -> Self {
        // Read the host-configured storage path off the un-started app so the
        // wallet runtime can install its durable payment store. SAFETY:
        // `self.app` is non-null (builder invariant) and not yet started, so a
        // shared borrow is sound.
        let storage_path = unsafe { &*self.app }.storage_path_for_start();
        // `register_wallet` takes `&mut impl AppHost`; the builder implements
        // `AppHost`, so it wires every registration against this builder's app.
        nmp_nip47::register_wallet(&mut self, storage_path);
        self
    }
}
