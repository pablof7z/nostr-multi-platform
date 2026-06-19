//! `nmp.wallet.connect` / `nmp.wallet.disconnect` action modules (V-38).
//!
//! Connection lifecycle modules extracted from `action/mod.rs` to keep it
//! under the 500-LOC file-size cap.

use serde::{Deserialize, Serialize};

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use nmp_core::ActorCommand;

use crate::protocol::{WalletConnectCommand, WalletDisconnectCommand};
use crate::runtime::WalletRuntimeHandle;

// ── WalletConnect ────────────────────────────────────────────────────────────

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

// ── WalletDisconnect ─────────────────────────────────────────────────────────

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

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::substrate::ActionContext;

    fn ctx() -> ActionContext {
        ActionContext::default()
    }

    fn handle() -> WalletRuntimeHandle {
        crate::runtime::new_wallet_runtime_handle()
    }

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
}
