//! ADR-0052 §D5 — the public adapter that bridges a `&mut Kernel` into the
//! narrow [`WalletKernelAccess`](crate::substrate::WalletKernelAccess) and
//! [`ZapProfileLookup`](crate::substrate::ZapProfileLookup) capabilities.
//!
//! Rung 5.5 deleted `ProtocolCommandContext::kernel_mut()` — the ambient
//! `&mut Kernel` escape hatch every boxed `ProtocolCommand` could reach. The
//! NIP-47 wallet runtime genuinely mutates eight kernel methods on the actor
//! thread, so those eight are promoted to the [`WalletKernelAccess`] trait, and
//! the zap-only cached-profile read becomes [`ZapProfileLookup`]. This adapter
//! is the single concrete impl of both: it wraps a `RefCell<&mut Kernel>` and
//! performs each mutation through a per-call `try_borrow_mut`, so it composes
//! with the dispatch arm's other capability adapters (which also hold the
//! kernel via `RefCell`) without a long-lived exclusive borrow.
//!
//! Two call paths construct it:
//!
//! 1. The actor's `Protocol(cmd)` dispatch arm — installs it as the context's
//!    `wallet_kernel` / `zap_profiles` capability so a wallet/zap
//!    `ProtocolCommand` reaches exactly its needed surface and nothing else.
//! 2. The wallet `RelayTextInterceptor` (`nmp_nip47::register`) — holds a real
//!    `&mut Kernel` directly and wraps it with [`Kernel::as_wallet_access`] to
//!    drive the same runtime helpers off the dispatch path.

use std::cell::RefCell;

use crate::kernel::Kernel;
use crate::substrate::{WalletKernelAccess, ZapProfileLookup};
use crate::{AuthSignerFn, RelayRole};

/// Adapter wrapping a `&mut Kernel` as the narrow wallet/zap capabilities.
///
/// Holds the kernel behind a `RefCell` so the `&self` capability methods can
/// take a transient `try_borrow_mut`. The reference never crosses a thread
/// boundary — it lives only for the actor-thread call that built it.
pub struct KernelWalletAccess<'a> {
    kernel: RefCell<&'a mut Kernel>,
}

impl<'a> KernelWalletAccess<'a> {
    /// Wrap `kernel` as the wallet/zap capability surface. Prefer
    /// [`Kernel::as_wallet_access`] at call sites that already hold the kernel.
    #[must_use]
    pub fn new(kernel: &'a mut Kernel) -> Self {
        Self {
            kernel: RefCell::new(kernel),
        }
    }
}

// SAFETY: the adapter is constructed and dropped on the actor thread; the
// `&mut Kernel` it wraps never crosses a thread boundary. The `Send + Sync`
// claim is required only because the substrate capability traits carry the
// bound (`dyn WalletKernelAccess` / `dyn ZapProfileLookup` live behind `&dyn`
// in `ProtocolCommandContext`). Mirrors the dispatch-arm capability adapters.
unsafe impl<'a> Send for KernelWalletAccess<'a> {}
unsafe impl<'a> Sync for KernelWalletAccess<'a> {}

impl<'a> WalletKernelAccess for KernelWalletAccess<'a> {
    fn now_secs(&self) -> u64 {
        self.kernel.try_borrow().map(|k| k.now_secs()).unwrap_or(0)
    }

    fn set_last_error_toast(&self, message: Option<String>) {
        if let Ok(mut k) = self.kernel.try_borrow_mut() {
            k.set_last_error_toast(message);
        }
    }

    fn record_action_failure(&self, correlation_id: String, reason: String) {
        if let Ok(mut k) = self.kernel.try_borrow_mut() {
            k.record_action_failure(correlation_id, reason);
        }
    }

    fn record_action_success(&self, correlation_id: String, result_json: Option<String>) {
        if let Ok(mut k) = self.kernel.try_borrow_mut() {
            k.record_action_success(correlation_id, result_json);
        }
    }

    fn set_relay_auth_signer(
        &self,
        role: RelayRole,
        pubkey_hex: String,
        signer: AuthSignerFn,
    ) {
        if let Ok(mut k) = self.kernel.try_borrow_mut() {
            k.set_relay_auth_signer(role, pubkey_hex, signer);
        }
    }

    fn clear_relay_auth_signer(&self, role: RelayRole) {
        if let Ok(mut k) = self.kernel.try_borrow_mut() {
            k.clear_relay_auth_signer(role);
        }
    }

    fn register_persistent_sub(&self, relay_url: String, sub_id: String) {
        if let Ok(mut k) = self.kernel.try_borrow_mut() {
            k.register_persistent_sub(relay_url, sub_id);
        }
    }

    fn unregister_persistent_sub(&self, relay_url: &str, sub_id: &str) {
        if let Ok(mut k) = self.kernel.try_borrow_mut() {
            k.unregister_persistent_sub(relay_url, sub_id);
        }
    }

    fn mark_changed_since_emit(&self) {
        if let Ok(mut k) = self.kernel.try_borrow_mut() {
            k.mark_changed_since_emit();
        }
    }
}

impl<'a> ZapProfileLookup for KernelWalletAccess<'a> {
    fn lnurl_for_pubkey(&self, pubkey: &str) -> Option<String> {
        self.kernel
            .try_borrow()
            .ok()
            .and_then(|k| k.lnurl_for_pubkey(pubkey))
    }
}

impl Kernel {
    /// ADR-0052 §D5 — wrap `self` as the narrow
    /// [`WalletKernelAccess`](crate::substrate::WalletKernelAccess) /
    /// [`ZapProfileLookup`](crate::substrate::ZapProfileLookup) capability
    /// surface for a single actor-thread call.
    ///
    /// The actor's wallet `RelayTextInterceptor` (which holds a real
    /// `&mut Kernel`) uses this to drive the `nmp-nip47` runtime helpers off
    /// the `ProtocolCommand` dispatch path; the helpers name only the narrow
    /// capability, so the two entry points share one runtime without either
    /// reaching the whole kernel.
    #[must_use]
    pub fn as_wallet_access(&mut self) -> KernelWalletAccess<'_> {
        KernelWalletAccess::new(self)
    }
}
