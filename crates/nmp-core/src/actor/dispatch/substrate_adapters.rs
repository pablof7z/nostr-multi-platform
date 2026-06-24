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
    fn set_last_error_token(&self, token: &crate::ui_token::UiToken) {
        if let Ok(mut k) = self.kernel.try_borrow_mut() {
            k.set_last_error_token(token);
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
    fn set_last_error_token(&self, token: &crate::ui_token::UiToken) {
        if let Ok(mut k) = self.kernel.try_borrow_mut() {
            k.set_last_error_token(token);
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
        role: nmp_network::role::RelayRole,
        pubkey_hex: String,
        signer: crate::AuthSignerFn,
    ) {
        if let Ok(mut k) = self.kernel.try_borrow_mut() {
            k.set_relay_auth_signer(role, pubkey_hex, signer);
        }
    }
    fn clear_relay_auth_signer(&self, role: nmp_network::role::RelayRole) {
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

/// #1940 — bridge the kernel's `local_write_relays` projection into the
/// substrate [`crate::substrate::WriteRelayLookup`] capability. Marmot's typed
/// protocol command consults this for `publish_key_package` / `create_group`
/// relay resolution without naming `NmpApp`. Kernel read only; `try_borrow`
/// keeps the adapter total under a re-entrant borrow.
pub(super) struct WriteRelayLookupAdapter<'a> {
    pub(super) kernel: &'a std::cell::RefCell<&'a mut Kernel>,
}

unsafe impl<'a> Send for WriteRelayLookupAdapter<'a> {}
unsafe impl<'a> Sync for WriteRelayLookupAdapter<'a> {}

impl<'a> crate::substrate::WriteRelayLookup for WriteRelayLookupAdapter<'a> {
    fn write_relay_urls(&self) -> Vec<String> {
        self.kernel
            .try_borrow()
            .ok()
            .and_then(|k| {
                k.local_write_relays_handle()
                    .lock()
                    .ok()
                    .map(|guard| guard.as_slice().to_vec())
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    //! Regression guard for #1364 (K2 rung-5.4 regression #1356), re-expressed
    //! against a generic `ProtocolCommand` after #1940 deleted the
    //! `HostOpCommand`/`HostOpHandler` seam.
    //!
    //! The whole-body `catch_unwind` wrapping a `ProtocolCommand` at the
    //! dispatch arm must NOT drop the `Requested` action-stage write. Before
    //! #1363 deleted the long-lived `with_kernel` exclusive borrow, the
    //! `ActionStageTrackerAdapter::record_requested` `try_borrow_mut` failed
    //! (the dispatch arm still held the `&mut Kernel`) and the write was
    //! silently dropped — a *pending* command (the Marmot KP-gated path) then
    //! had NO `action_stages` entry until its async continuation fired, so the
    //! host could not tell "pending, awaiting continuation" from "dropped".
    //!
    //! This test exercises the REAL `ActionStageTrackerAdapter` against a REAL
    //! kernel through a tiny pending `ProtocolCommand` that records `Requested`
    //! and then defers (records no terminal) — the exact shape the Marmot
    //! `MarmotProtocolCommand` takes on a KP-gated `{"pending":true}` op. It
    //! asserts the kernel's `action_stages` projection carries a `Requested`
    //! entry — the durable oracle that the panic-guarded command path records
    //! its Requested stage like every other action path.

    use crate::actor::ActorCommand;
    use crate::kernel::Kernel;
    use crate::relay::DEFAULT_VISIBLE_LIMIT;
    use crate::substrate::{
        EmptyDmInboxRelayLookup, NoopErrorSurface, NoopKernelClock, NoopLocalSignerAccess,
        NoopRecipientRelayLookup, NoopWalletKernelAccess, NoopWriteRelayLookup,
        NoopZapProfileLookup, ProtocolCommand, ProtocolCommandContext, ProtocolCommandContextParts,
        ProtocolCommandError,
    };
    use std::cell::RefCell;

    /// A tiny pending `ProtocolCommand` mirroring the Marmot KP-gated path: it
    /// records `Requested` and then defers (no terminal verdict written here).
    #[derive(Debug)]
    struct PendingCommand {
        correlation_id: String,
    }

    impl ProtocolCommand for PendingCommand {
        fn run(
            self: Box<Self>,
            ctx: &mut ProtocolCommandContext<'_>,
        ) -> Result<(), ProtocolCommandError> {
            ctx.record_action_stage_requested(&self.correlation_id);
            // Deferred: no terminal verdict recorded (the `{"pending":true}`
            // branch of `MarmotProtocolCommand::run`).
            Ok(())
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
    fn pending_command_records_requested_stage_through_real_adapter() {
        // A real kernel, wrapped in the SAME `RefCell<&mut Kernel>` shape the
        // dispatch arm builds, so the adapter's `try_borrow_mut` is exercised
        // exactly as in production.
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let correlation_id = "corr-marmot-pending";

        {
            let kernel_cell = RefCell::new(&mut kernel);
            let stages = super::ActionStageTrackerAdapter {
                kernel: &kernel_cell,
            };

            // Noop surfaces for every capability the command does not touch.
            static CLOCK: NoopKernelClock = NoopKernelClock;
            static SIGNERS: NoopLocalSignerAccess = NoopLocalSignerAccess;
            static ERRORS: NoopErrorSurface = NoopErrorSurface;
            static RECIPIENTS: NoopRecipientRelayLookup = NoopRecipientRelayLookup;
            static WALLET: NoopWalletKernelAccess = NoopWalletKernelAccess;
            static ZAP: NoopZapProfileLookup = NoopZapProfileLookup;
            static DMS: EmptyDmInboxRelayLookup = EmptyDmInboxRelayLookup;
            static WRITE_RELAYS: NoopWriteRelayLookup = NoopWriteRelayLookup;

            let (tx, _rx) = std::sync::mpsc::channel::<crate::actor::ActorMail>();
            let command_sender = crate::actor::CommandSender::new(tx);
            // The terminal verdict would re-enter via `send`; a pending command
            // sends nothing.
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
                wallet_kernel: &WALLET,
                zap_profiles: &ZAP,
                write_relays: &WRITE_RELAYS,
            });

            Box::new(PendingCommand {
                correlation_id: correlation_id.to_string(),
            })
            .run(&mut ctx)
            .expect("PendingCommand::run never returns Err");
        }

        // ORACLE: a pending command MUST leave a `Requested` action-stage entry
        // so the host can tell "pending, awaiting continuation" from "dropped".
        let history = stage_history(&mut kernel, correlation_id);
        let arr = history
            .as_array()
            .expect("pending command must have an action_stages history entry (#1364)");
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
