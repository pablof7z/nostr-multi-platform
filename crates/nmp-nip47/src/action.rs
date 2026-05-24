//! NIP-47 wallet `ActionModule`s — the `nmp.wallet.*` namespaces routed
//! through `nmp_app_dispatch_action`.
//!
//! Moved from `nmp-core::wallet::action` (V-38). The module is unchanged
//! from a host's perspective: namespace `nmp.wallet.pay_invoice` stays
//! byte-stable, the `WalletAction` serde shape is locked by the
//! `wallet_action_round_trips_through_serde` test below.
//!
//! What changed: `execute()` no longer emits a bespoke
//! `ActorCommand::WalletPayInvoice` variant (deleted in V-38). It emits
//! `ActorCommand::Protocol(Box::new(WalletPayInvoiceCommand{...}))` so the
//! kernel ships no NIP-47 nouns in its `ActorCommand` enum (D0).

use serde::{Deserialize, Serialize};

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use nmp_core::ActorCommand;

use crate::protocol::{
    WalletConnectCommand, WalletDisconnectCommand, WalletPayInvoiceCommand,
};

/// User-initiated wallet intents dispatchable through
/// `nmp_app_dispatch_action` under the `nmp.wallet.pay_invoice` namespace.
///
/// `PayInvoice` is currently the only variant: connection lifecycle
/// (`wallet_connect` / `wallet_disconnect`) stays on dedicated FFI symbols
/// per the Theme A discriminator (see module docs).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum WalletAction {
    /// Pay a BOLT-11 Lightning invoice via the connected NIP-47 wallet.
    PayInvoice {
        bolt11: String,
        amount_msats: Option<u64>,
    },
}

// ── Connection lifecycle action modules (V-38) ──────────────────────────────

/// Wire shape for `nmp.wallet.connect` — parse a NWC URI and bring the
/// runtime up. Single-field externally-tagged enum so the wire JSON shape is
/// `{"Connect":{"uri":"nostr+walletconnect://…"}}`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum WalletConnectAction {
    Connect { uri: String },
}

/// `ActionModule` for `nmp.wallet.connect`. Replaces the pre-V-38 bespoke
/// `nmp_app_wallet_connect` FFI symbol's direct ActorCommand construction.
pub struct WalletConnectModule;

impl ActionModule for WalletConnectModule {
    const NAMESPACE: &'static str = "nmp.wallet.connect";
    type Action = WalletConnectAction;

    fn start(_ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        match action {
            WalletConnectAction::Connect { uri } => {
                if uri.is_empty() {
                    return Err(ActionRejection::Invalid(
                        "wallet connect requires a non-empty NWC URI".to_string(),
                    ));
                }
                Ok(())
            }
        }
    }

    fn execute(
        action: Self::Action,
        _correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        let Some(runtime) = crate::runtime::active_wallet_runtime() else {
            return Err(
                "wallet runtime not installed — host must call nmp_nip47::install_wallet_runtime"
                    .to_string(),
            );
        };
        match action {
            WalletConnectAction::Connect { uri } => {
                send(ActorCommand::Protocol(Box::new(WalletConnectCommand {
                    uri,
                    runtime,
                })));
                Ok(())
            }
        }
    }
}

/// Wire shape for `nmp.wallet.disconnect`. Unit variant (no payload).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum WalletDisconnectAction {
    Disconnect,
}

/// `ActionModule` for `nmp.wallet.disconnect`.
pub struct WalletDisconnectModule;

impl ActionModule for WalletDisconnectModule {
    const NAMESPACE: &'static str = "nmp.wallet.disconnect";
    type Action = WalletDisconnectAction;

    fn execute(
        _action: Self::Action,
        _correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        let Some(runtime) = crate::runtime::active_wallet_runtime() else {
            return Err(
                "wallet runtime not installed — host must call nmp_nip47::install_wallet_runtime"
                    .to_string(),
            );
        };
        send(ActorCommand::Protocol(Box::new(WalletDisconnectCommand {
            runtime,
        })));
        Ok(())
    }
}

// ── nmp.wallet.pay_invoice ──────────────────────────────────────────────────

/// `ActionModule` implementation for `nmp.wallet.pay_invoice`.
pub struct WalletPayInvoiceModule;

impl ActionModule for WalletPayInvoiceModule {
    const NAMESPACE: &'static str = "nmp.wallet.pay_invoice";

    type Action = WalletAction;

    /// Validate the action shape. `bolt11` must be non-empty.
    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<(), ActionRejection> {
        match action {
            WalletAction::PayInvoice { bolt11, .. } => {
                if bolt11.is_empty() {
                    return Err(ActionRejection::Invalid(
                        "wallet pay_invoice requires a non-empty bolt11 invoice".to_string(),
                    ));
                }
                Ok(())
            }
        }
    }

    fn is_async_completing() -> bool { // doctrine-allow: D12 — recording sites are cross-file (`runtime.rs` `record_action_success`/`record_action_failure`); covered by runtime tests
        true
    }

    /// Translate the validated action into a [`WalletPayInvoiceCommand`]
    /// wrapped in [`ActorCommand::Protocol`].
    ///
    /// Pre-V-38 this emitted the bespoke `ActorCommand::WalletPayInvoice`
    /// variant; V-38 deleted that variant — the open `Protocol` seam is
    /// the substrate-generic replacement.
    fn execute(
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        let Some(runtime) = crate::runtime::active_wallet_runtime() else {
            return Err(
                "wallet runtime not installed — host must call nmp_nip47::install_wallet_runtime"
                    .to_string(),
            );
        };
        match action {
            WalletAction::PayInvoice { bolt11, amount_msats } => {
                let cmd = WalletPayInvoiceCommand {
                    bolt11,
                    amount_msats,
                    correlation_id: Some(correlation_id.to_string()),
                    runtime,
                };
                send(ActorCommand::Protocol(Box::new(cmd)));
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ActionContext {
        ActionContext::default()
    }

    #[test]
    fn start_accepts_non_empty_bolt11() {
        let action = WalletAction::PayInvoice {
            bolt11: "lnbc100n1p0fakeinvoice".to_string(),
            amount_msats: None,
        };
        WalletPayInvoiceModule::start(&mut ctx(), action)
            .expect("non-empty bolt11 must be accepted");
    }

    #[test]
    fn start_accepts_explicit_amount_msats() {
        let action = WalletAction::PayInvoice {
            bolt11: "lnbc1p0amountless".to_string(),
            amount_msats: Some(21_000),
        };
        WalletPayInvoiceModule::start(&mut ctx(), action)
            .expect("explicit amount must be accepted");
    }

    #[test]
    fn start_rejects_empty_bolt11() {
        let action = WalletAction::PayInvoice {
            bolt11: String::new(),
            amount_msats: None,
        };
        let err = WalletPayInvoiceModule::start(&mut ctx(), action)
            .expect_err("empty bolt11 must be rejected");
        match err {
            ActionRejection::Invalid(msg) => {
                assert!(
                    msg.contains("non-empty bolt11"),
                    "rejection should explain the constraint: {msg}"
                );
            }
            other => panic!("expected Invalid rejection, got {other:?}"),
        }
    }

    #[test]
    fn is_async_completing_is_true() {
        assert!(
            WalletPayInvoiceModule::is_async_completing(),
            "pay_invoice settles asynchronously via the kind:23195 response"
        );
    }

    /// Locks in the externally-tagged enum representation
    /// (`{"PayInvoice":{...}}`) as the wire shape any caller — Rust, Swift,
    /// Kotlin — must produce. Byte-stable from the pre-V-38 surface.
    #[test]
    fn wallet_action_round_trips_through_serde() {
        let action = WalletAction::PayInvoice {
            bolt11: "lnbc100n1p0roundtrip".to_string(),
            amount_msats: Some(42),
        };
        let json = serde_json::to_string(&action).expect("serialize must succeed");
        assert!(
            json.contains("\"PayInvoice\""),
            "externally-tagged enum shape must include the variant name: {json}"
        );
        let decoded: WalletAction =
            serde_json::from_str(&json).expect("deserialize must succeed");
        assert_eq!(action, decoded, "round-trip must preserve the value");
    }

    /// `execute` emits exactly one `Protocol`-wrapped `WalletPayInvoiceCommand`
    /// carrying the registry-minted `correlation_id`.
    ///
    /// Requires the process-wide runtime handle to be installed first. The
    /// `OnceLock` means this test races with sibling tests in the same
    /// process — guarded by `install_wallet_runtime`'s `Err`-on-double-set
    /// semantic, so we tolerate a "already installed" rejection here.
    #[test]
    fn execute_emits_protocol_wrapped_pay_invoice_command() {
        use std::cell::RefCell;

        let handle = crate::runtime::new_wallet_runtime_handle();
        // OK if a sibling test installed first — only the shape matters.
        let _ = crate::runtime::install_wallet_runtime(handle);

        let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
        let action = WalletAction::PayInvoice {
            bolt11: "lnbc500n1p0testinvoice".to_string(),
            amount_msats: Some(1_234),
        };
        let minted_correlation_id = "be".repeat(16);

        WalletPayInvoiceModule::execute(action, &minted_correlation_id, &|cmd| {
            captured.borrow_mut().push(cmd);
        })
        .expect("execute must succeed");

        let cmds = captured.into_inner();
        assert_eq!(cmds.len(), 1, "executor must emit exactly one ActorCommand");
        match cmds.into_iter().next().unwrap() {
            ActorCommand::Protocol(_) => {
                // Body content is verified through the runtime; here we only
                // assert the variant shape so the kernel's NIP-noun count
                // stays zero.
            }
            other => panic!("expected ActorCommand::Protocol, got {other:?}"),
        }
    }
}
