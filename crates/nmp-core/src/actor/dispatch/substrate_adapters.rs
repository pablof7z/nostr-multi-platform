//! Debt C — capability adapters for `ProtocolCommandContext`.
//!
//! The `Protocol(cmd)` dispatch arm constructs these to bridge the actor's
//! kernel + identity references into the typed capability traits the substrate
//! `ProtocolCommandContext` consumes. Lifetimes are bound to the dispatch arm's
//! stack frame; the adapters never outlive their `RefCell` borrow targets.
//!
//! Extracted from `dispatch.rs` so that file stays within its LOC ceiling — the
//! adapters are a self-contained cluster of `'a`-lifetime substrate-trait
//! wrappers over a `&RefCell<&mut Kernel>` / `&RefCell<&IdentityRuntime>`.
//!
//! #1927 — the wallet/zap surface (`WalletKernelAccessAdapter` /
//! `ZapProfileLookupAdapter`) used to be duplicated here; both were byte-for-byte
//! copies of [`crate::kernel::wallet_access::KernelWalletAccess`]. They are
//! deleted: the dispatch arm now constructs `KernelWalletAccess::borrowed` over
//! the shared `kernel_cell`, so there is one audited wallet/zap impl.

use super::IdentityRuntime;
use crate::kernel::Kernel;

/// #1927 — record a dropped capability mutation loudly. A failed
/// `try_borrow_mut` on the shared `kernel_cell` means a re-entrant kernel
/// borrow leaked across `cmd.run` (a bug); in debug it panics so tests catch
/// it, in release it stays a logged no-op so the in-flight command survives.
fn record_dropped_mutation(what: &str) {
    debug_assert!(
        false,
        "dispatch adapter kernel borrow contended during {what}"
    );
    tracing::error!(op = what, "dispatch adapter mutation dropped: borrow contended");
}

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
        match self.kernel.try_borrow_mut() {
            Ok(mut k) => k.set_last_error_toast(message),
            Err(_) => record_dropped_mutation("set_last_error_toast"),
        }
    }
    fn set_last_error_token(&self, token: &crate::ui_token::UiToken) {
        match self.kernel.try_borrow_mut() {
            Ok(mut k) => k.set_last_error_token(token),
            Err(_) => record_dropped_mutation("set_last_error_token"),
        }
    }
    fn record_action_failure(&self, correlation_id: String, reason: String) {
        match self.kernel.try_borrow_mut() {
            Ok(mut k) => k.record_action_failure(correlation_id, reason),
            Err(_) => record_dropped_mutation("record_action_failure"),
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
        match self.kernel.try_borrow_mut() {
            Ok(mut k) => k.record_action_stage(
                correlation_id,
                crate::kernel::action_stages::ActionStage::Requested,
                None,
            ),
            Err(_) => record_dropped_mutation("record_requested"),
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

/// ADR-0052 §D4 — bridge the actor's snapped per-app host-op handler into the
/// substrate [`crate::substrate::HostOpHandlerAccess`] capability. Reaches no
/// kernel/identity state.
pub(super) struct HostOpHandlerAccessAdapter {
    pub(super) handler: Option<std::sync::Arc<dyn crate::substrate::HostOpHandler>>,
}

unsafe impl Send for HostOpHandlerAccessAdapter {}
unsafe impl Sync for HostOpHandlerAccessAdapter {}

impl crate::substrate::HostOpHandlerAccess for HostOpHandlerAccessAdapter {
    fn current_handler(&self) -> Option<std::sync::Arc<dyn crate::substrate::HostOpHandler>> {
        self.handler.clone()
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
    ///
    /// Takes `&mut Kernel` because `action_stages_projection` drives the
    /// `note_copy_emit` Cleared-edge machine (ADR-0055 Rung 3 S1b §10.4).
    fn stage_history(kernel: &mut Kernel, correlation_id: &str) -> serde_json::Value {
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
        let history = stage_history(&mut kernel, correlation_id);
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
