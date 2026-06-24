//! ADR-0052 §D5 — the single adapter that bridges a `Kernel` into the narrow
//! [`WalletKernelAccess`](crate::substrate::WalletKernelAccess) and
//! [`ZapProfileLookup`](crate::substrate::ZapProfileLookup) capabilities.
//!
//! Rung 5.5 deleted `ProtocolCommandContext::kernel_mut()` — the ambient
//! `&mut Kernel` escape hatch every boxed `ProtocolCommand` could reach. The
//! NIP-47 wallet runtime genuinely mutates eight kernel methods on the actor
//! thread, so those eight are promoted to the [`WalletKernelAccess`] trait, and
//! the zap-only cached-profile read becomes [`ZapProfileLookup`].
//!
//! #1927 — this is now the ONLY concrete impl of both traits. The prior
//! `WalletKernelAccessAdapter` / `ZapProfileLookupAdapter` in
//! `dispatch/substrate_adapters.rs` were byte-identical duplicates; they are
//! deleted and both call paths converge here via [`KernelCell`]:
//!
//! 1. The actor's `Protocol(cmd)` dispatch arm — calls
//!    [`KernelWalletAccess::borrowed`] to share the SAME `RefCell<&mut Kernel>`
//!    the sibling read adapters hold, so a wallet command's mutations interleave
//!    with the other capability reads through one cell (no long-lived exclusive
//!    borrow).
//! 2. The wallet `RelayTextInterceptor` (`nmp_nip47::register`) — holds a real
//!    `&mut Kernel` directly and wraps it with [`Kernel::as_wallet_access`]
//!    ([`KernelWalletAccess::new`]) to drive the same runtime helpers off the
//!    dispatch path.

use std::cell::RefCell;

use crate::kernel::Kernel;
use crate::substrate::{WalletKernelAccess, ZapProfileLookup};
use crate::AuthSignerFn;
use nmp_network::role::RelayRole;

/// Either an owned `RefCell<&mut Kernel>` (off-dispatch callers that hold the
/// kernel directly) or a borrowed `&RefCell<&mut Kernel>` shared with the
/// dispatch arm's sibling capability adapters. Both modes route every method
/// through one [`KernelCell::cell`] accessor, so there is a single set of trait
/// impls.
enum KernelCell<'a> {
    Owned(RefCell<&'a mut Kernel>),
    Borrowed(&'a RefCell<&'a mut Kernel>),
}

impl<'a> KernelCell<'a> {
    fn cell(&self) -> &RefCell<&'a mut Kernel> {
        match self {
            KernelCell::Owned(c) => c,
            KernelCell::Borrowed(c) => c,
        }
    }
}

/// #1927 — run `f` against the kernel via a transient `try_borrow_mut`.
///
/// In production a failed borrow stays a no-op: the dispatch arm holds no
/// long-lived borrow (the prior `with_kernel` exclusive borrow was deleted), so
/// this never fires. In `debug_assertions` a failed borrow is a bug
/// (re-entrant kernel access) and panics, so tests catch a dropped mutation
/// instead of it silently vanishing.
fn with_kernel_mut(cell: &RefCell<&mut Kernel>, what: &str, f: impl FnOnce(&mut Kernel)) {
    match cell.try_borrow_mut() {
        Ok(mut k) => f(&mut k),
        Err(_) => {
            debug_assert!(false, "wallet kernel borrow contended during {what}");
            tracing::error!(
                op = what,
                "wallet kernel mutation dropped: borrow contended"
            );
        }
    }
}

/// Adapter wrapping a `Kernel` as the narrow wallet/zap capabilities.
///
/// Holds the kernel behind a [`KernelCell`] so the `&self` capability methods
/// can take a transient `try_borrow_mut`. The reference never crosses a thread
/// boundary — it lives only for the actor-thread call that built it.
pub struct KernelWalletAccess<'a> {
    kernel: KernelCell<'a>,
}

impl<'a> KernelWalletAccess<'a> {
    /// Wrap an owned `&mut Kernel`. Used off the dispatch path
    /// (`nmp_nip47::register`'s `RelayTextInterceptor`). Prefer
    /// [`Kernel::as_wallet_access`] at call sites that already hold the kernel.
    #[must_use]
    pub fn new(kernel: &'a mut Kernel) -> Self {
        Self {
            kernel: KernelCell::Owned(RefCell::new(kernel)),
        }
    }

    /// #1927 — wrap a `RefCell<&mut Kernel>` already shared with the dispatch
    /// arm's sibling read adapters, so the wallet/zap surface composes through
    /// the SAME cell (no second `&mut Kernel` alias). Crate-internal: only the
    /// `Protocol(cmd)` dispatch arm constructs this mode.
    #[must_use]
    pub(crate) fn borrowed(cell: &'a RefCell<&'a mut Kernel>) -> Self {
        Self {
            kernel: KernelCell::Borrowed(cell),
        }
    }
}

impl<'a> WalletKernelAccess for KernelWalletAccess<'a> {
    fn now_secs(&self) -> u64 {
        self.kernel
            .cell()
            .try_borrow()
            .map(|k| k.now_secs())
            .unwrap_or(0)
    }

    fn set_last_error_toast(&self, message: Option<String>) {
        with_kernel_mut(self.kernel.cell(), "set_last_error_toast", |k| {
            k.set_last_error_toast(message)
        });
    }

    fn set_last_error_token(&self, token: &crate::ui_token::UiToken) {
        with_kernel_mut(self.kernel.cell(), "set_last_error_token", |k| {
            k.set_last_error_token(token)
        });
    }

    fn record_action_failure(&self, correlation_id: String, reason: String) {
        with_kernel_mut(self.kernel.cell(), "record_action_failure", |k| {
            k.record_action_failure(correlation_id, reason)
        });
    }

    fn record_action_success(&self, correlation_id: String, result_json: Option<String>) {
        with_kernel_mut(self.kernel.cell(), "record_action_success", |k| {
            k.record_action_success(correlation_id, result_json)
        });
    }

    fn set_relay_auth_signer(&self, role: RelayRole, pubkey_hex: String, signer: AuthSignerFn) {
        with_kernel_mut(self.kernel.cell(), "set_relay_auth_signer", |k| {
            k.set_relay_auth_signer(role, pubkey_hex, signer)
        });
    }

    fn clear_relay_auth_signer(&self, role: RelayRole) {
        with_kernel_mut(self.kernel.cell(), "clear_relay_auth_signer", |k| {
            k.clear_relay_auth_signer(role)
        });
    }

    fn register_persistent_sub(&self, relay_url: String, sub_id: String) {
        with_kernel_mut(self.kernel.cell(), "register_persistent_sub", |k| {
            k.register_persistent_sub(relay_url, sub_id)
        });
    }

    fn unregister_persistent_sub(&self, relay_url: &str, sub_id: &str) {
        with_kernel_mut(self.kernel.cell(), "unregister_persistent_sub", |k| {
            k.unregister_persistent_sub(relay_url, sub_id)
        });
    }

    fn mark_changed_since_emit(&self) {
        with_kernel_mut(self.kernel.cell(), "mark_changed_since_emit", |k| {
            k.mark_changed_since_emit()
        });
    }
}

impl<'a> ZapProfileLookup for KernelWalletAccess<'a> {
    fn lnurl_for_pubkey(&self, pubkey: &str) -> Option<String> {
        self.kernel
            .cell()
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

#[cfg(test)]
mod tests {
    //! #1927 — regression guard for the unified wallet/zap adapter.
    //!
    //! Covers both ownership modes (`Owned` via `as_wallet_access`, `Borrowed`
    //! via `borrowed`), the zap-profile lookup read, and the borrow-contention
    //! behaviour (total no-op in release, `debug_assert!` panic in debug).

    use super::super::nostr::NostrEvent;
    use super::KernelWalletAccess;
    use crate::kernel::Kernel;
    use crate::relay::DEFAULT_VISIBLE_LIMIT;
    use crate::substrate::{WalletKernelAccess, ZapProfileLookup};
    use std::cell::RefCell;

    /// Read `action_stages.<correlation_id>` straight from the kernel's
    /// projection (the wire surface the host observes), returning the stage
    /// history array or `Null` when absent.
    fn stage_history(kernel: &mut Kernel, correlation_id: &str) -> serde_json::Value {
        kernel
            .action_stages_projection()
            .get(correlation_id)
            .cloned()
            .unwrap_or(serde_json::Value::Null)
    }

    fn has_failure_stage(history: &serde_json::Value) -> bool {
        history
            .as_array()
            .map(|arr| {
                arr.iter().any(|e| {
                    e.get("stage")
                        .and_then(serde_json::Value::as_str)
                        .map(|s| s.eq_ignore_ascii_case("failed"))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    }

    #[test]
    fn borrowed_and_owned_modes_mutate_same_kernel() {
        // (a) Owned mode via `Kernel::as_wallet_access`.
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        {
            let access = kernel.as_wallet_access();
            access.record_action_failure("corr-owned".into(), "boom".into());
        }
        assert!(
            has_failure_stage(&stage_history(&mut kernel, "corr-owned")),
            "owned-mode mutation must reach the kernel"
        );

        // (b) Borrowed mode via `KernelWalletAccess::borrowed`, sharing one cell.
        {
            let cell = RefCell::new(&mut kernel);
            let access = KernelWalletAccess::borrowed(&cell);
            access.record_action_failure("corr-borrowed".into(), "boom".into());
        }
        assert!(
            has_failure_stage(&stage_history(&mut kernel, "corr-borrowed")),
            "borrowed-mode mutation must reach the same kernel"
        );
    }

    #[test]
    fn zap_profile_lookup_reads_cached_kind0() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let pk = "0000000000000000000000000000000000000000000000000000000000000001";
        // Seed a kind:0 carrying an `lud16` lightning address. `inject_profile`
        // is the kernel's test-support kind:0 ingest seam (`parse_profile`
        // reads only `content`).
        let event = NostrEvent {
            id: "1".repeat(64),
            pubkey: pk.to_string(),
            created_at: 1_700_000_000,
            kind: 0,
            tags: Vec::new(),
            content: r#"{"name":"alice","lud16":"alice@example.com"}"#.to_string(),
            sig: String::new(),
        };
        kernel.inject_profile(event);

        let access = kernel.as_wallet_access();
        assert_eq!(
            access.lnurl_for_pubkey(pk),
            Some("alice@example.com".to_string()),
            "zap lookup must surface the cached kind:0 lightning address"
        );
        assert_eq!(
            access.lnurl_for_pubkey(&"f".repeat(64)),
            None,
            "unknown pubkey must return None"
        );
    }

    #[test]
    #[cfg(not(debug_assertions))]
    fn borrow_contention_drops_mutation_in_release() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let cell = RefCell::new(&mut kernel);
        let access = KernelWalletAccess::borrowed(&cell);
        // Hold a conflicting shared borrow so the mutation's try_borrow_mut fails.
        let guard = cell.borrow();
        access.record_action_failure("corr-contended".into(), "boom".into());
        drop(guard);
        // Mutation was dropped (no panic), kernel unchanged.
        assert!(
            !has_failure_stage(&stage_history(&mut kernel, "corr-contended")),
            "contended mutation must be a total no-op in release"
        );
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "borrow contended")]
    fn borrow_contention_panics_in_debug() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let cell = RefCell::new(&mut kernel);
        let access = KernelWalletAccess::borrowed(&cell);
        let _guard = cell.borrow();
        // The `debug_assert!` in `with_kernel_mut` fires on the contended borrow.
        access.record_action_failure("corr-contended".into(), "boom".into());
    }
}
