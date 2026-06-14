//! Debt C — capability adapters for `ProtocolCommandContext`.
//!
//! The `Protocol(cmd)` dispatch arm constructs these to bridge the actor's
//! kernel + identity references into the typed capability traits the substrate
//! `ProtocolCommandContext` consumes. Lifetimes are bound to the dispatch arm's
//! stack frame; the adapters never outlive their `RefCell` borrow targets.
//!
//! Extracted from `dispatch.rs` so that file stays within its LOC ceiling — the
//! adapters are a self-contained cluster (five `'a`-lifetime substrate-trait
//! wrappers over a `&RefCell<&mut Kernel>` / `&RefCell<&IdentityRuntime>`).

use super::IdentityRuntime;
use crate::kernel::Kernel;

pub(super) struct KernelClockAdapter<'a> {
    pub(super) kernel: &'a std::cell::RefCell<&'a mut Kernel>,
}

// SAFETY: the dispatch arm constructs and drops the adapter on the actor
// thread; the `&RefCell<&mut Kernel>` reference never crosses a thread
// boundary. The `Send + Sync` claim is needed because the substrate trait
// carries the bound (`dyn KernelClock` lives behind `&dyn` in
// `ProtocolCommandContext`), but the adapter is held only for the dispatch
// arm's stack frame.
unsafe impl<'a> Send for KernelClockAdapter<'a> {}
unsafe impl<'a> Sync for KernelClockAdapter<'a> {}

impl<'a> crate::substrate::KernelClock for KernelClockAdapter<'a> {
    fn now_secs(&self) -> u64 {
        self.kernel.borrow().now_secs()
    }
}

pub(super) struct LocalSignerAccessAdapter<'a> {
    pub(super) identity: &'a std::cell::RefCell<&'a IdentityRuntime>,
}

unsafe impl<'a> Send for LocalSignerAccessAdapter<'a> {}
unsafe impl<'a> Sync for LocalSignerAccessAdapter<'a> {}

impl<'a> crate::substrate::LocalSignerAccess for LocalSignerAccessAdapter<'a> {
    fn active_local_keys(&self) -> Option<nostr::Keys> {
        self.identity.borrow().active_local_keys().cloned()
    }
    fn active_account_pubkey(&self) -> Option<String> {
        self.identity.borrow().active_pubkey()
    }
}

pub(super) struct ErrorSurfaceAdapter<'a> {
    pub(super) kernel: &'a std::cell::RefCell<&'a mut Kernel>,
}

unsafe impl<'a> Send for ErrorSurfaceAdapter<'a> {}
unsafe impl<'a> Sync for ErrorSurfaceAdapter<'a> {}

impl<'a> crate::substrate::ErrorSurface for ErrorSurfaceAdapter<'a> {
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
}

pub(super) struct ActionStageTrackerAdapter<'a> {
    pub(super) kernel: &'a std::cell::RefCell<&'a mut Kernel>,
}

unsafe impl<'a> Send for ActionStageTrackerAdapter<'a> {}
unsafe impl<'a> Sync for ActionStageTrackerAdapter<'a> {}

impl<'a> crate::substrate::ActionStageTracker for ActionStageTrackerAdapter<'a> {
    fn record_requested(&self, correlation_id: &str) {
        if let Ok(mut k) = self.kernel.try_borrow_mut() {
            k.record_action_stage(
                correlation_id,
                crate::kernel::action_stages::ActionStage::Requested,
                None,
            );
        }
    }
}

/// Debt-C-follow-up — bridge the kernel's `outbox_router` slot into the
/// substrate [`crate::substrate::RecipientRelayLookup`] capability. NIP-57
/// LNURL fetcher consumes this to populate the kind:9734 `relays` tag
/// (recipient's NIP-65 write set + cold-start fallback) without naming
/// `OutboxRouter` or the substrate `MailboxCache` directly.
pub(super) struct RecipientRelayLookupAdapter<'a> {
    pub(super) kernel: &'a std::cell::RefCell<&'a mut Kernel>,
}

unsafe impl<'a> Send for RecipientRelayLookupAdapter<'a> {}
unsafe impl<'a> Sync for RecipientRelayLookupAdapter<'a> {}

impl<'a> crate::substrate::RecipientRelayLookup for RecipientRelayLookupAdapter<'a> {
    fn recipient_publish_relays(&self, recipient: &str, kind: u32) -> Vec<String> {
        // Kernel read; no mutation required. `try_borrow` keeps the adapter
        // total in the presence of a re-entrant kernel borrow on the dispatch
        // arm (defensive — production has no such cycle).
        self.kernel
            .try_borrow()
            .ok()
            .map(|k| k.recipient_publish_relays(recipient, kind))
            .unwrap_or_default()
    }
}

/// ADR-0052 §D5 — bridge the actor's `&mut Kernel` into the narrow
/// [`crate::substrate::WalletKernelAccess`] capability (the eight kernel
/// methods the NIP-47 wallet runtime mutates on the actor thread). Replaces the
/// deleted `ProtocolCommandContext::kernel_mut()` escape hatch: a wallet
/// command can drive these eight and nothing else. Each method takes a
/// transient `try_borrow_mut` so it composes with the sibling read adapters
/// (`KernelClockAdapter` etc.) that share the same `RefCell<&mut Kernel>`
/// across `cmd.run` — no long-lived exclusive borrow.
pub(super) struct WalletKernelAccessAdapter<'a> {
    pub(super) kernel: &'a std::cell::RefCell<&'a mut Kernel>,
}

unsafe impl<'a> Send for WalletKernelAccessAdapter<'a> {}
unsafe impl<'a> Sync for WalletKernelAccessAdapter<'a> {}

impl<'a> crate::substrate::WalletKernelAccess for WalletKernelAccessAdapter<'a> {
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
        role: crate::RelayRole,
        pubkey_hex: String,
        signer: crate::AuthSignerFn,
    ) {
        if let Ok(mut k) = self.kernel.try_borrow_mut() {
            k.set_relay_auth_signer(role, pubkey_hex, signer);
        }
    }
    fn clear_relay_auth_signer(&self, role: crate::RelayRole) {
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

/// ADR-0052 §D5 — bridge the actor's `&mut Kernel` into the narrow
/// [`crate::substrate::ZapProfileLookup`] capability (the zap-only cached-kind:0
/// lightning-address read). Replaces the generic `lnurl_for_pubkey` accessor.
/// Kernel read only; `try_borrow` keeps the adapter total under a re-entrant
/// borrow.
pub(super) struct ZapProfileLookupAdapter<'a> {
    pub(super) kernel: &'a std::cell::RefCell<&'a mut Kernel>,
}

unsafe impl<'a> Send for ZapProfileLookupAdapter<'a> {}
unsafe impl<'a> Sync for ZapProfileLookupAdapter<'a> {}

impl<'a> crate::substrate::ZapProfileLookup for ZapProfileLookupAdapter<'a> {
    fn lnurl_for_pubkey(&self, pubkey: &str) -> Option<String> {
        self.kernel
            .try_borrow()
            .ok()
            .and_then(|k| k.lnurl_for_pubkey(pubkey))
    }
}

/// ADR-0052 §D4 — bridge the actor's per-app
/// [`crate::substrate::HostOpHandlerSlot`] into the substrate
/// [`crate::substrate::HostOpHandlerAccess`] capability so the
/// [`crate::substrate::HostOpCommand`] can clone the currently-installed
/// handler out of the slot at `run` time (honouring account-switch hot-swaps).
/// Reaches no kernel/identity state — only the handler slot.
pub(super) struct HostOpHandlerAccessAdapter<'a> {
    pub(super) slot: &'a crate::substrate::HostOpHandlerSlot,
}

unsafe impl<'a> Send for HostOpHandlerAccessAdapter<'a> {}
unsafe impl<'a> Sync for HostOpHandlerAccessAdapter<'a> {}

impl<'a> crate::substrate::HostOpHandlerAccess for HostOpHandlerAccessAdapter<'a> {
    fn current_handler(
        &self,
    ) -> Option<std::sync::Arc<dyn crate::substrate::HostOpHandler>> {
        // Clone the inner `Arc` under the slot lock and return by value so the
        // (SQLite-bound) `handle` call never holds the slot mutex (D8 — must
        // not block the FFI `set_host_op_handler` writer).
        self.slot.lock().ok().and_then(|guard| guard.as_ref().cloned())
    }
}

#[cfg(test)]
mod tests {
    //! Regression guard for #1364 (K2 rung-5.4 regression #1356).
    //!
    //! The whole-body `catch_unwind` wrapping a `HostOpCommand` at the dispatch
    //! arm must NOT drop the `Requested` action-stage write. Before #1363
    //! deleted the long-lived `with_kernel` exclusive borrow, the
    //! `ActionStageTrackerAdapter::record_requested` `try_borrow_mut` failed
    //! (the dispatch arm still held the `&mut Kernel`) and the write was
    //! silently dropped — a Marmot/MLS *pending* host op then had NO
    //! `action_stages` entry until its async continuation fired, so the host
    //! could not tell "pending, awaiting KP fetch" from "silently dropped".
    //!
    //! This test exercises the REAL `ActionStageTrackerAdapter` against a REAL
    //! kernel through the REAL `HostOpCommand::run`, with a handler that returns
    //! `{"pending":true}` (the Marmot KP-gated path). It asserts the kernel's
    //! `action_stages` projection carries a `Requested` entry — the durable
    //! oracle that the panic-guarded host-op path records its Requested stage
    //! like every other action path.

    use crate::actor::ActorCommand;
    use crate::kernel::Kernel;
    use crate::relay::DEFAULT_VISIBLE_LIMIT;
    use crate::substrate::{
        host_op_command, EmptyDmInboxRelayLookup, HostOpHandler, HostOpHandlerAccess,
        NoopErrorSurface, NoopKernelClock, NoopLocalSignerAccess, NoopRecipientRelayLookup,
        NoopWalletKernelAccess, NoopZapProfileLookup, ProtocolCommand, ProtocolCommandContext,
        ProtocolCommandContextParts,
    };
    use std::cell::RefCell;
    use std::sync::{Arc, Mutex};

    /// Handler mirroring the Marmot KP-gated MLS op: it defers completion, so
    /// only the `Requested` stage is written synchronously.
    struct PendingHandler;
    impl HostOpHandler for PendingHandler {
        fn handle(&self, _: &str, _: &str) -> serde_json::Value {
            serde_json::json!({ "pending": true })
        }
    }

    struct SlotAccess(Arc<Mutex<Option<Arc<dyn HostOpHandler>>>>);
    impl HostOpHandlerAccess for SlotAccess {
        fn current_handler(&self) -> Option<Arc<dyn HostOpHandler>> {
            self.0.lock().ok().and_then(|g| g.as_ref().cloned())
        }
    }

    /// Read `action_stages.<correlation_id>` straight from the kernel's
    /// projection (the wire surface the host observes), returning the stage
    /// history array or `Null` when absent.
    fn stage_history(kernel: &Kernel, correlation_id: &str) -> serde_json::Value {
        kernel
            .action_stages_projection()
            .get(correlation_id)
            .cloned()
            .unwrap_or(serde_json::Value::Null)
    }

    #[test]
    fn pending_host_op_records_requested_stage_through_real_adapter() {
        // A real kernel, wrapped in the SAME `RefCell<&mut Kernel>` shape the
        // dispatch arm builds, so the adapter's `try_borrow_mut` is exercised
        // exactly as in production.
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let correlation_id = "corr-marmot-pending";

        // Install the pending handler in a real slot accessor.
        let slot = crate::substrate::new_host_op_handler_slot();
        *slot.lock().unwrap() = Some(Arc::new(PendingHandler) as Arc<dyn HostOpHandler>);
        let access = SlotAccess(slot);

        {
            let kernel_cell = RefCell::new(&mut kernel);
            let stages = super::ActionStageTrackerAdapter {
                kernel: &kernel_cell,
            };

            // Noop surfaces for every capability the host op does not touch.
            static CLOCK: NoopKernelClock = NoopKernelClock;
            static SIGNERS: NoopLocalSignerAccess = NoopLocalSignerAccess;
            static ERRORS: NoopErrorSurface = NoopErrorSurface;
            static RECIPIENTS: NoopRecipientRelayLookup = NoopRecipientRelayLookup;
            static WALLET: NoopWalletKernelAccess = NoopWalletKernelAccess;
            static ZAP: NoopZapProfileLookup = NoopZapProfileLookup;
            static DMS: EmptyDmInboxRelayLookup = EmptyDmInboxRelayLookup;

            let (tx, _rx) = std::sync::mpsc::channel::<crate::actor::ActorMail>();
            let command_sender = crate::actor::CommandSender::new(tx);
            // The host op's terminal verdict re-enters via `send`; a pending op
            // sends nothing, but the slot must exist.
            let send: &dyn Fn(ActorCommand) = &|_c: ActorCommand| {};

            let mut ctx = ProtocolCommandContext::new(ProtocolCommandContextParts {
                send,
                command_sender,
                clock: &CLOCK,
                signers: &SIGNERS,
                dms: &DMS,
                errors: &ERRORS,
                stages: &stages,
                recipients: &RECIPIENTS,
                host_op_handler: &access,
                wallet_kernel: &WALLET,
                zap_profiles: &ZAP,
            });

            Box::new(host_op_command("{}".into(), correlation_id.into()))
                .run(&mut ctx)
                .expect("HostOpCommand::run never returns Err");
        }

        // ORACLE: a pending host op MUST leave a `Requested` action-stage entry
        // so the host can tell "pending, awaiting continuation" from "dropped".
        let history = stage_history(&kernel, correlation_id);
        let arr = history
            .as_array()
            .expect("pending host op must have an action_stages history entry (#1364)");
        assert!(
            arr.iter().any(|e| {
                e.get("stage")
                    .and_then(serde_json::Value::as_str)
                    .map(|s| s.eq_ignore_ascii_case("requested"))
                    .unwrap_or(false)
            }),
            "expected a 'Requested' stage entry, got {history:?}"
        );
    }
}
