//! `HostOpCommand` — the [`ProtocolCommand`] that expresses a stateful,
//! host-owned op dispatch through the single write seam.
//!
//! # Why this exists (ADR-0052 §D4 — K2 rung 5.4)
//!
//! Before rung 5.4 there were **two** `ActorCommand` write seams for the same
//! concern (a host runs a write op on the actor thread, panic-isolated, with a
//! snapshot-projected result): `ActorCommand::Protocol(Box<dyn
//! ProtocolCommand>)` and a bespoke `ActorCommand::DispatchHostOp { .. }` arm.
//! The latter pulled a persistent, app-installed `Arc<dyn HostOpHandler>` from
//! a per-app slot, wrapped `handle()` in `catch_unwind`, and routed the
//! `{"ok"/"pending"/"error"}` envelope into the `action_stages` /
//! `action_results` mirror.
//!
//! Rung 5.4 collapses the two doors into one. The persistent installed handler
//! stays exactly where it was — the per-app
//! [`HostOpHandlerSlot`](crate::substrate::HostOpHandlerSlot) the host writes
//! through [`crate::NmpApp::set_host_op_handler`]. What changes is how the
//! *dispatch* is expressed: instead of a dedicated `ActorCommand` variant, each
//! host op mints a fresh [`HostOpCommand`] (a one-shot `ProtocolCommand`) that,
//! at `run` time, asks the per-app slot for an `Arc::clone` of whatever handler
//! is installed *now* — through the narrow
//! [`HostOpHandlerAccess`](crate::substrate::HostOpHandlerAccess) capability on
//! [`ProtocolCommandContext`]. Because the clone is taken at run time, an
//! account-switch hot-swap of the handler is still honoured (D2).
//!
//! # The two guarantees this preserves
//!
//! 1. **Whole-body panic isolation.** The `Protocol` dispatch arm now wraps the
//!    entire `cmd.run(..)` in `catch_unwind` (added in this rung), so a panic
//!    anywhere in this command's body — including inside `handle()` — converts
//!    to the same `{"ok":false,..}` error surface rather than unwinding the
//!    actor thread. This body additionally wraps `handle()` directly so the
//!    *failure-record* still names the panic precisely even though the
//!    arm-level catch would also stop the unwind.
//! 2. **Persistent handler, one-shot command.** The handler lives in the per-app
//!    slot (persistent, hot-swappable); only the command is consumed per op.
//!
//! # Terminal-verdict routing (D8, single canonical path)
//!
//! `run` records `Requested` through [`ProtocolCommandContext::record_action_stage_requested`]
//! and then routes the handler envelope by **re-entering the actor loop** with
//! the existing [`ActorCommand::RecordActionSuccess`] /
//! [`ActorCommand::RecordActionFailure`] commands — the same terminal-recording
//! path off-thread workers and the deferred `{"pending":true}` continuation
//! already use. There is exactly one terminal-recording mechanism after this
//! rung; the host (which polls the `action_stages` snapshot projection) cannot
//! observe the one-tick re-entry deferral. `{"pending":true}` leaves the action
//! in `Requested` and records nothing — the handler later sends its own
//! `RecordAction*` command (callback-driven, no polling, D8-safe).

use crate::substrate::protocol::{
    ProtocolCommand, ProtocolCommandContext, ProtocolCommandError,
};
use crate::ActorCommand;

/// One-shot dispatch of a host-owned op to the per-app
/// [`HostOpHandler`](crate::substrate::HostOpHandler).
///
/// Construct via [`host_op_command`]; enqueue as
/// `ActorCommand::Protocol(Box::new(host_op_command(action_json, correlation_id)))`.
/// Replaces the deleted `ActorCommand::DispatchHostOp { action_json,
/// correlation_id }` variant (ADR-0052 §D4).
#[derive(Debug)]
pub struct HostOpCommand {
    /// JSON-encoded action body. The handler parses this into its own typed
    /// action enum. No protocol type crosses this boundary (D0).
    action_json: String,
    /// Registry-minted dispatch correlation id (32 hex chars). Threaded into
    /// the handler and into the `action_stages` terminal verdict.
    correlation_id: String,
}

/// Build a [`HostOpCommand`] for `action_json` under `correlation_id`. The
/// producer is an `ActionModule::execute` body in an app crate (today
/// `nmp-app-marmot`'s `MarmotActionModule`) that serialises its typed action to
/// JSON; the handler installed by the same crate parses it back out.
#[must_use]
pub fn host_op_command(action_json: String, correlation_id: String) -> HostOpCommand {
    HostOpCommand {
        action_json,
        correlation_id,
    }
}

impl ProtocolCommand for HostOpCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let HostOpCommand {
            action_json,
            correlation_id,
        } = *self;

        // Record `Requested` first so the host's spinner sees the action
        // entered the actor lane even if the handler is absent or panics
        // (mirrors the legacy `DispatchHostOp` arm and the V-41 LNURL command).
        ctx.record_action_stage_requested(&correlation_id);

        // Pull the handler clone OUT of the per-app slot before calling
        // `handle` so the slot mutex is not held across the (SQLite-bound) work
        // (D8). `None` ⇒ no stateful app bound.
        let handler = ctx.host_op_handler();
        let result = match handler {
            Some(handler) => {
                // D6 — wrap the host-side `handle()` in `catch_unwind` so a
                // buggy handler that panics maps to a `{"ok":false,..}` failure
                // surface with a precise reason. (The `Protocol` arm now ALSO
                // wraps the whole body, but recording the failure here keeps
                // the terminal verdict accurate rather than relying on the
                // arm's coarser fallback.)
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    handler.handle(&action_json, &correlation_id)
                }))
                .unwrap_or_else(|_| {
                    serde_json::json!({
                        "ok": false,
                        "error": "host op handler panicked"
                    })
                })
            }
            None => serde_json::json!({
                "ok": false,
                "error": "no host op handler installed"
            }),
        };

        // Route the envelope to the action_results/action_stages mirror via the
        // single terminal-recording path (RecordAction* re-entry).
        // `{"pending":true}` means the handler deferred completion (e.g. a
        // Marmot KP-gated op awaiting a key-package fetch): leave the action in
        // its already-written `Requested` stage and let the handler push a
        // later `RecordActionSuccess`/`RecordActionFailure` (D8-safe — no
        // timer, no polling). Otherwise `{"ok":true}` records success and
        // anything else records failure (static reason fallback).
        let flag = |k| {
            result
                .get(k)
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        };
        if flag("pending") {
            // Deferred path owns the terminal write; nothing to record now.
        } else if flag("ok") {
            // Host-op success carries no structured result body (D0).
            ctx.send(ActorCommand::RecordActionSuccess {
                correlation_id,
                result_json: None,
            });
        } else {
            let reason = result
                .get("error")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("host op failed without an error message")
                .to_string();
            ctx.send(ActorCommand::RecordActionFailure {
                correlation_id,
                reason,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::substrate::protocol::{
        HostOpHandlerAccess, ProtocolCommandContext, ProtocolCommandContextParts,
    };
    use crate::substrate::HostOpHandler;
    use std::cell::RefCell;
    use std::sync::{Arc, Mutex};

    /// Capture every `ActorCommand` the command re-enters with, so the tests
    /// can assert the terminal verdict it routed.
    fn captured_ctx<'a>(
        sink: &'a RefCell<Vec<ActorCommand>>,
        access: &'a dyn HostOpHandlerAccess,
    ) -> ProtocolCommandContext<'a> {
        use crate::substrate::protocol::{
            NoopActionStageTracker, NoopErrorSurface, NoopKernelClock, NoopLocalSignerAccess,
            NoopRecipientRelayLookup,
        };
        static CLOCK: NoopKernelClock = NoopKernelClock;
        static SIGNERS: NoopLocalSignerAccess = NoopLocalSignerAccess;
        static ERRORS: NoopErrorSurface = NoopErrorSurface;
        static STAGES: NoopActionStageTracker = NoopActionStageTracker;
        static RECIPIENTS: NoopRecipientRelayLookup = NoopRecipientRelayLookup;
        static WALLET: crate::substrate::NoopWalletKernelAccess =
            crate::substrate::NoopWalletKernelAccess;
        static ZAP: crate::substrate::NoopZapProfileLookup =
            crate::substrate::NoopZapProfileLookup;
        static DMS: crate::substrate::EmptyDmInboxRelayLookup =
            crate::substrate::EmptyDmInboxRelayLookup;
        let (command_sender, _rx) = std::sync::mpsc::channel::<crate::actor::ActorMail>();
        let command_sender = crate::actor::CommandSender::new(command_sender);
        // The `send` closure pushes into the sink.
        let send: &'a dyn Fn(ActorCommand) = Box::leak(Box::new(move |c: ActorCommand| {
            sink.borrow_mut().push(c);
        }));
        ProtocolCommandContext::new(ProtocolCommandContextParts {
            send,
            command_sender,
            clock: &CLOCK,
            signers: &SIGNERS,
            dms: &DMS,
            errors: &ERRORS,
            stages: &STAGES,
            recipients: &RECIPIENTS,
            host_op_handler: access,
            wallet_kernel: &WALLET,
            zap_profiles: &ZAP,
        })
    }

    /// A handler installed in a slot-backed [`HostOpHandlerAccess`].
    struct SlotAccess(Arc<Mutex<Option<Arc<dyn HostOpHandler>>>>);
    impl HostOpHandlerAccess for SlotAccess {
        fn current_handler(&self) -> Option<Arc<dyn HostOpHandler>> {
            self.0.lock().ok().and_then(|g| g.as_ref().cloned())
        }
    }

    struct OkHandler;
    impl HostOpHandler for OkHandler {
        fn handle(&self, _: &str, _: &str) -> serde_json::Value {
            serde_json::json!({ "ok": true })
        }
    }

    struct PanicHandler;
    impl HostOpHandler for PanicHandler {
        fn handle(&self, _: &str, _: &str) -> serde_json::Value {
            panic!("handler exploded");
        }
    }

    struct PendingHandler;
    impl HostOpHandler for PendingHandler {
        fn handle(&self, _: &str, _: &str) -> serde_json::Value {
            serde_json::json!({ "pending": true })
        }
    }

    fn run(access: &dyn HostOpHandlerAccess, sink: &RefCell<Vec<ActorCommand>>) {
        let mut ctx = captured_ctx(sink, access);
        Box::new(host_op_command("{}".into(), "corr-1".into()))
            .run(&mut ctx)
            .expect("HostOpCommand::run never returns Err");
    }

    #[test]
    fn ok_handler_routes_record_action_success() {
        let slot = crate::substrate::new_host_op_handler_slot();
        *slot.lock().unwrap() = Some(Arc::new(OkHandler) as Arc<dyn HostOpHandler>);
        let access = SlotAccess(slot);
        let sink = RefCell::new(Vec::new());
        run(&access, &sink);
        let cmds = sink.into_inner();
        assert_eq!(cmds.len(), 1, "expected exactly one terminal command");
        match &cmds[0] {
            ActorCommand::RecordActionSuccess {
                correlation_id,
                result_json,
            } => {
                assert_eq!(correlation_id, "corr-1");
                assert!(result_json.is_none(), "host-op success carries no body");
            }
            other => panic!("expected RecordActionSuccess, got {other:?}"),
        }
    }

    /// ORACLE (guarantee #2): a panicking handler does NOT unwind `run`; it is
    /// converted to a `RecordActionFailure` terminal naming the panic.
    #[test]
    fn panicking_handler_is_caught_and_records_failure() {
        let slot = crate::substrate::new_host_op_handler_slot();
        *slot.lock().unwrap() = Some(Arc::new(PanicHandler) as Arc<dyn HostOpHandler>);
        let access = SlotAccess(slot);
        let sink = RefCell::new(Vec::new());
        // `run` returning at all proves the panic was caught (did not unwind).
        run(&access, &sink);
        let cmds = sink.into_inner();
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            ActorCommand::RecordActionFailure {
                correlation_id,
                reason,
            } => {
                assert_eq!(correlation_id, "corr-1");
                assert_eq!(reason, "host op handler panicked");
            }
            other => panic!("expected RecordActionFailure, got {other:?}"),
        }
    }

    #[test]
    fn no_handler_records_failure() {
        let access = crate::substrate::protocol::NoopHostOpHandlerAccess;
        let sink = RefCell::new(Vec::new());
        run(&access, &sink);
        let cmds = sink.into_inner();
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            ActorCommand::RecordActionFailure { reason, .. } => {
                assert_eq!(reason, "no host op handler installed");
            }
            other => panic!("expected RecordActionFailure, got {other:?}"),
        }
    }

    /// A soft `{"ok":false,"error":..}` envelope routes the error verbatim.
    #[test]
    fn soft_failure_routes_error_message() {
        struct SoftFail;
        impl HostOpHandler for SoftFail {
            fn handle(&self, _: &str, _: &str) -> serde_json::Value {
                serde_json::json!({ "ok": false, "error": "key_package_unavailable" })
            }
        }
        let slot = crate::substrate::new_host_op_handler_slot();
        *slot.lock().unwrap() = Some(Arc::new(SoftFail) as Arc<dyn HostOpHandler>);
        let access = SlotAccess(slot);
        let sink = RefCell::new(Vec::new());
        run(&access, &sink);
        match &sink.into_inner()[0] {
            ActorCommand::RecordActionFailure { reason, .. } => {
                assert_eq!(reason, "key_package_unavailable");
            }
            other => panic!("expected RecordActionFailure, got {other:?}"),
        }
    }

    /// `{"pending":true}` records NO terminal — the deferred continuation owns
    /// it (D8 callback-driven).
    #[test]
    fn pending_records_nothing() {
        let slot = crate::substrate::new_host_op_handler_slot();
        *slot.lock().unwrap() = Some(Arc::new(PendingHandler) as Arc<dyn HostOpHandler>);
        let access = SlotAccess(slot);
        let sink = RefCell::new(Vec::new());
        run(&access, &sink);
        assert!(
            sink.into_inner().is_empty(),
            "pending must not record a terminal verdict now"
        );
    }
}
