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
    /// The wallet runtime must be wired before any `nmp.wallet.*` action
    /// dispatches. ADR-0072 rung 5.2: the three wallet `ActionModule`s are
    /// registered BY VALUE, each owning a clone of the per-app
    /// `WalletRuntimeHandle` `nmp_nip47::register` creates — there is no
    /// process-global. Previously the wiring lived in an app crate
    /// (`nmp-app-chirp::wallet_runtime`) with no compile-time ordering
    /// guarantee.
    ///
    /// Folding the wiring into the builder makes it a *config-phase* step:
    /// because `start()` consumes the builder by move, a Rust caller cannot
    /// reach `start()` without `.with_wallet()` having already registered the
    /// modules. The ordering contract is expressed in the type system.
    ///
    /// The durable [`FsPaymentStore`](nmp_nip47::FsPaymentStore) is installed
    /// automatically when a persistent storage path is configured — i.e. when
    /// `.storage_path(p)` was called before this step (or
    /// `nmp_app_set_storage_path` was driven through the C-ABI). With
    /// `.in_memory()` (no path), the runtime tracks payments in memory only.
    ///
    /// Per-instance (ADR-0072 rung 5.2): two builders in one process now get
    /// two INDEPENDENT wallet runtimes (no shared global), so a second
    /// `.with_wallet()` wires a distinct runtime rather than silently yielding.
    /// The returned per-app handle is threaded into the NIP-57 zap auto-chain
    /// through `nmp_nip57::Config::with_payment_port`, so a zap pays through
    /// THIS app's wallet.
    ///
    /// # Errors
    ///
    /// Returns `Err(RegistrationError)` if either the NIP-47 wallet actions or
    /// the NIP-57 zap action collide with an already-registered action on this
    /// builder — mirrors the fallible idiom every other registration step on
    /// this builder uses (see `ActionRegistrar::register_action`).
    pub fn with_wallet(mut self) -> Result<Self, nmp_core::substrate::RegistrationError> {
        // Read the host-configured storage path off the un-started app so the
        // wallet runtime can install its durable payment store. SAFETY:
        // `self.app` is non-null (builder invariant) and not yet started, so a
        // shared borrow is sound.
        let storage_path = unsafe { &*self.app }.storage_path_for_start();
        // `nmp_nip47::register` takes only the narrow registrar traits it uses; the
        // builder implements `AppHost` (the composition supertrait over them), so
        // it satisfies those bounds and wires every registration against its app.
        // It returns the per-app `WalletRuntimeHandle` (ADR-0072 rung 5.2).
        let wallet_handles = nmp_nip47::register(&mut self, nmp_nip47::Config::new(storage_path))?;
        // Inject a NIP-47-backed `PaymentPort` into the NIP-57 zap auto-chain:
        // the app-path override of the port-less zap default `explicit owner composition`
        // installs (ADR-0069), so a zap pays through this builder's wallet. The
        // `nmp-nip57 → nmp-nip47` edge is gone — NIP-57 sees only the substrate
        // `PaymentPort` (#1728), and `explicit composition` (composition) wires the two.
        nmp_nip57::register(
            &mut self,
            nmp_nip57::Config::with_payment_port(nmp_nip47::wallet_payment_port(
                wallet_handles.wallet,
            )),
        )?;
        Ok(self)
    }
}
