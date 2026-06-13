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
use crate::runtime::WalletRuntimeHandle;

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
///
/// ADR-0052 rung 5.2: owns its `WalletRuntimeHandle` by value (cloned from
/// the composition root). `execute` reaches the runtime through `self.runtime`
/// — no process-global. Two `NmpApp` instances therefore drive independent
/// runtimes.
pub struct WalletConnectModule {
    pub runtime: WalletRuntimeHandle,
}

impl WalletConnectModule {
    /// Construct the module bound to `runtime` (the per-app handle the
    /// composition root cloned for this seam).
    #[must_use]
    pub fn new(runtime: WalletRuntimeHandle) -> Self {
        Self { runtime }
    }
}

impl ActionModule for WalletConnectModule {
    const NAMESPACE: &'static str = "nmp.wallet.connect";
    type Action = WalletConnectAction;

    /// Validate the NWC URI before the runtime attempts to parse it.
    ///
    /// V-100: URI scheme validation lives here, not in the Swift shell
    /// (thin-shell doctrine). The Connect button always dispatches; if the
    /// URI fails validation the action is rejected synchronously and the FFI
    /// shim surfaces the reason as a `last_error_toast`.
    ///
    /// Validates:
    /// 1. Non-empty URI
    /// 2. `nostr+walletconnect://` scheme prefix (case-insensitive)
    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        match action {
            WalletConnectAction::Connect { uri } => {
                if uri.is_empty() {
                    return Err(ActionRejection::Invalid(
                        "wallet connect requires a non-empty NWC URI".to_string(),
                    ));
                }
                if !uri.to_ascii_lowercase().starts_with("nostr+walletconnect://") {
                    return Err(ActionRejection::Invalid(
                        "invalid NWC URI: must start with nostr+walletconnect://".to_string(),
                    ));
                }
                Ok(())
            }
        }
    }

    fn execute(
        &self,
        action: Self::Action,
        _correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        match action {
            WalletConnectAction::Connect { uri } => {
                send(ActorCommand::Protocol(Box::new(WalletConnectCommand {
                    uri,
                    runtime: self.runtime.clone(),
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
///
/// ADR-0052 rung 5.2: owns its per-app `WalletRuntimeHandle` by value.
pub struct WalletDisconnectModule {
    pub runtime: WalletRuntimeHandle,
}

impl WalletDisconnectModule {
    /// Construct the module bound to `runtime` (the per-app handle).
    #[must_use]
    pub fn new(runtime: WalletRuntimeHandle) -> Self {
        Self { runtime }
    }
}

impl ActionModule for WalletDisconnectModule {
    const NAMESPACE: &'static str = "nmp.wallet.disconnect";
    type Action = WalletDisconnectAction;

    fn execute(
        &self,
        _action: Self::Action,
        _correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(ActorCommand::Protocol(Box::new(WalletDisconnectCommand {
            runtime: self.runtime.clone(),
        })));
        Ok(())
    }
}

// ── nmp.wallet.pay_invoice ──────────────────────────────────────────────────

/// `ActionModule` implementation for `nmp.wallet.pay_invoice`.
///
/// ADR-0052 rung 5.2: owns its per-app `WalletRuntimeHandle` by value, so the
/// pay request reaches THIS app's wallet runtime (no process-global). This is
/// what makes two `NmpApp` instances pay through independent wallets.
pub struct WalletPayInvoiceModule {
    pub runtime: WalletRuntimeHandle,
}

impl WalletPayInvoiceModule {
    /// Construct the module bound to `runtime` (the per-app handle).
    #[must_use]
    pub fn new(runtime: WalletRuntimeHandle) -> Self {
        Self { runtime }
    }
}

impl ActionModule for WalletPayInvoiceModule {
    const NAMESPACE: &'static str = "nmp.wallet.pay_invoice";

    type Action = WalletAction;

    /// Validate the action shape. `bolt11` must be non-empty.
    fn start(
        &self,
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
        &self,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        match action {
            WalletAction::PayInvoice { bolt11, amount_msats } => {
                let cmd = WalletPayInvoiceCommand {
                    bolt11,
                    amount_msats,
                    correlation_id: Some(correlation_id.to_string()),
                    runtime: self.runtime.clone(),
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

    /// A fresh, empty per-instance handle for unit tests. ADR-0052 rung 5.2:
    /// each module owns its handle, so tests no longer touch a process-global
    /// (the prior `OnceLock` install path that self-admittedly raced sibling
    /// tests is gone).
    fn handle() -> WalletRuntimeHandle {
        crate::runtime::new_wallet_runtime_handle()
    }

    // ── WalletConnectModule tests ─────────────────────────────────────────

    #[test]
    fn connect_start_accepts_valid_nwc_uri() {
        let action = WalletConnectAction::Connect {
            uri: "nostr+walletconnect://abc123?relay=wss://relay.example.com&secret=xyz".to_string(),
        };
        WalletConnectModule::new(handle())
            .start(&mut ctx(), action)
            .expect("valid nostr+walletconnect:// URI must be accepted");
    }

    #[test]
    fn connect_start_accepts_nwc_uri_mixed_case_scheme() {
        // The scheme check is case-insensitive; uppercase variants should pass.
        let action = WalletConnectAction::Connect {
            uri: "NOSTR+WALLETCONNECT://abc123?relay=wss://relay.example.com".to_string(),
        };
        WalletConnectModule::new(handle())
            .start(&mut ctx(), action)
            .expect("upper-case scheme variant must be accepted");
    }

    #[test]
    fn connect_start_rejects_empty_uri() {
        let action = WalletConnectAction::Connect {
            uri: String::new(),
        };
        let err = WalletConnectModule::new(handle())
            .start(&mut ctx(), action)
            .expect_err("empty URI must be rejected");
        match err {
            ActionRejection::Invalid(msg) => {
                assert!(msg.contains("non-empty"), "rejection should explain the constraint: {msg}");
            }
            other => panic!("expected Invalid rejection, got {other:?}"),
        }
    }

    #[test]
    fn connect_start_rejects_wrong_scheme() {
        for bad in &[
            "https://example.com",
            "nostr://abc",
            "walletconnect://abc",
            "lightning:abc",
            "   ",
        ] {
            let action = WalletConnectAction::Connect { uri: bad.to_string() };
            let err = WalletConnectModule::new(handle())
                .start(&mut ctx(), action)
                .expect_err(&format!("bad URI {bad:?} must be rejected"));
            match err {
                ActionRejection::Invalid(msg) => {
                    assert!(
                        msg.contains("nostr+walletconnect://"),
                        "rejection message must name the required scheme; got: {msg}"
                    );
                }
                other => panic!("expected Invalid for {bad:?}, got {other:?}"),
            }
        }
    }

    // ── WalletPayInvoiceModule tests ──────────────────────────────────────

    #[test]
    fn start_accepts_non_empty_bolt11() {
        let action = WalletAction::PayInvoice {
            bolt11: "lnbc100n1p0fakeinvoice".to_string(),
            amount_msats: None,
        };
        WalletPayInvoiceModule::new(handle())
            .start(&mut ctx(), action)
            .expect("non-empty bolt11 must be accepted");
    }

    #[test]
    fn start_accepts_explicit_amount_msats() {
        let action = WalletAction::PayInvoice {
            bolt11: "lnbc1p0amountless".to_string(),
            amount_msats: Some(21_000),
        };
        WalletPayInvoiceModule::new(handle())
            .start(&mut ctx(), action)
            .expect("explicit amount must be accepted");
    }

    #[test]
    fn start_rejects_empty_bolt11() {
        let action = WalletAction::PayInvoice {
            bolt11: String::new(),
            amount_msats: None,
        };
        let err = WalletPayInvoiceModule::new(handle())
            .start(&mut ctx(), action)
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
    /// ADR-0052 rung 5.2: the module owns its own handle, so this test no
    /// longer installs a process-global and CANNOT race a sibling test — the
    /// self-admitted `OnceLock` race documented here pre-rung is gone. Each
    /// test constructs an independent module value.
    #[test]
    fn execute_emits_protocol_wrapped_pay_invoice_command() {
        use std::cell::RefCell;

        let module = WalletPayInvoiceModule::new(handle());

        let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
        let action = WalletAction::PayInvoice {
            bolt11: "lnbc500n1p0testinvoice".to_string(),
            amount_msats: Some(1_234),
        };
        let minted_correlation_id = "be".repeat(16);

        module
            .execute(action, &minted_correlation_id, &|cmd| {
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
