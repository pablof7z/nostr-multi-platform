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
//!
//! #1607 — the bolt11 double-tap dedup guard that previously lived in the
//! FFI layer (`nmp-ffi::wallet::NmpApp::inflight_bolt11`) was moved into
//! [`WalletPayInvoiceModule`]. The guard is per-module-instance (ADR-0072 rung
//! 5.2: owned by value, no process-global), so two `NmpApp` instances in one
//! process dedup independently. The FFI shims `nmp_app_wallet_*` were deleted;
//! callers use `nmp_app_dispatch_action("nmp.wallet.*", …)` directly.
//!
//! # Submodule layout
//!
//! * `dedupe` — [`InflightBolt11Guard`](dedupe::InflightBolt11Guard) +
//!   [`INFLIGHT_BOLT11_TTL`] constant.
//! * `connect` — [`WalletConnectAction`] / [`WalletConnectModule`] /
//!   [`WalletDisconnectAction`] / [`WalletDisconnectModule`].
//! * This file (`mod.rs`) — [`WalletAction`] + [`WalletPayInvoiceModule`].

mod connect;
mod dedupe;

pub use connect::{
    WalletConnectAction, WalletConnectModule, WalletDisconnectAction, WalletDisconnectModule,
};
use dedupe::InflightBolt11Guard;
pub use dedupe::INFLIGHT_BOLT11_TTL;

use serde::{Deserialize, Serialize};

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRejection,
};

use crate::protocol::WalletPayInvoiceCommand;
use crate::runtime::WalletRuntimeHandle;

/// User-initiated wallet intents dispatchable through
/// `nmp_app_dispatch_action` under the `nmp.wallet.pay_invoice` namespace.
///
/// `PayInvoice` is currently the only variant: connection lifecycle
/// (`wallet_connect` / `wallet_disconnect`) stays on dedicated modules
/// in the `connect` submodule per the Theme A discriminator (see module docs).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum WalletAction {
    /// Pay a BOLT-11 Lightning invoice via the connected NIP-47 wallet.
    PayInvoice {
        bolt11: String,
        amount_msats: Option<u64>,
    },
}

// ── nmp.wallet.pay_invoice ──────────────────────────────────────────────────

/// `ActionModule` implementation for `nmp.wallet.pay_invoice`.
///
/// ADR-0072 rung 5.2: owns its per-app `WalletRuntimeHandle` by value, so the
/// pay request reaches THIS app's wallet runtime (no process-global). This is
/// what makes two `NmpApp` instances pay through independent wallets.
///
/// #1607: the UI-layer bolt11 double-tap guard (`inflight_bolt11`) that
/// previously lived in `nmp-ffi::NmpApp` is now owned here by value —
/// the same ADR-0072 rung 5.2 principle that removed the wallet runtime
/// process-global. Two `NmpApp` instances therefore dedup independently;
/// neither can observe the other's in-flight invoices.
pub struct WalletPayInvoiceModule {
    pub runtime: WalletRuntimeHandle,
    /// UI-layer bolt11 dedup guard. See [`dedupe::InflightBolt11Guard`] for
    /// the TTL, sweep, and D6/D8 policies.
    inflight_bolt11: InflightBolt11Guard,
}

impl WalletPayInvoiceModule {
    /// Construct the module bound to `runtime` (the per-app handle).
    #[must_use]
    pub fn new(runtime: WalletRuntimeHandle) -> Self {
        Self {
            runtime,
            inflight_bolt11: InflightBolt11Guard::new(),
        }
    }
}

impl ActionModule for WalletPayInvoiceModule {
    const NAMESPACE: nmp_core::substrate::DeclaredActionNamespace =
        nmp_core::substrate::DeclaredActionNamespace::framework(
            "nmp.wallet.pay_invoice",
            "action.nmp.wallet.pay_invoice",
        );

    type Action = WalletAction;

    /// Typed FlatBuffers payload decode (ADR-0071 / #1756) — delegates to the
    /// `nmp.wallet.pay_invoice` `ActionPayload` codec (`N47P`). The registry
    /// adapter runs the fail-closed `schema_version` gate BEFORE `start()`.
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<Self::Action as ActionPayload>::decode(bytes))
    }

    /// Validate the action shape. `bolt11` must be non-empty.
    ///
    /// Also enforces the UI-layer double-tap guard: a same-`bolt11` retap
    /// within [`INFLIGHT_BOLT11_TTL`] of the first is rejected with
    /// `ActionRejection::Conflict` so the host can surface a "payment in
    /// progress" state rather than a user-visible error. The guard is
    /// per-module-instance (no process-global) and sweeps expired entries
    /// lazily on each call (D8).
    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        match &action {
            WalletAction::PayInvoice { bolt11, .. } => {
                if bolt11.is_empty() {
                    return Err(ActionRejection::Invalid(
                        "wallet pay_invoice requires a non-empty bolt11 invoice".to_string(),
                    ));
                }
                if self.inflight_bolt11.is_duplicate_tap(bolt11) {
                    return Err(ActionRejection::Conflict(
                        "payment already in progress for this invoice".to_string(),
                    ));
                }
                Ok(())
            }
        }
    }

    #[rustfmt::skip]
    fn is_async_completing() -> bool { // doctrine-allow: D12 — recording sites are cross-file (`runtime/commands.rs`, `reconcile.rs` `kernel.record_action_failure`); covered by runtime tests
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
        _ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        match action {
            WalletAction::PayInvoice {
                bolt11,
                amount_msats,
            } => {
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

    /// A fresh, empty per-instance handle for unit tests. ADR-0072 rung 5.2:
    /// each module owns its handle, so tests no longer touch a process-global
    /// (the prior `OnceLock` install path that self-admittedly raced sibling
    /// tests is gone).
    fn handle() -> WalletRuntimeHandle {
        crate::runtime::new_wallet_runtime_handle()
    }

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
        let decoded: WalletAction = serde_json::from_str(&json).expect("deserialize must succeed");
        assert_eq!(action, decoded, "round-trip must preserve the value");
    }

    /// `execute` emits exactly one `Protocol`-wrapped `WalletPayInvoiceCommand`
    /// carrying the registry-minted `correlation_id`.
    ///
    /// ADR-0072 rung 5.2: the module owns its own handle, so this test no
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
            .execute(
                &nmp_core::substrate::ActionContext::default(),
                action,
                &minted_correlation_id,
                &|cmd| {
                    captured.borrow_mut().push(cmd);
                },
            )
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

    // ── Double-tap guard tests (#1607) ────────────────────────────────────

    /// The first dispatch of a bolt11 is accepted; the second within TTL is
    /// rejected as Conflict.
    #[test]
    fn same_bolt11_twice_second_is_conflict() {
        let module = WalletPayInvoiceModule::new(handle());
        let bolt11 = "lnbc100n1p0doubletap".to_string();

        let first = module.start(
            &mut ctx(),
            WalletAction::PayInvoice {
                bolt11: bolt11.clone(),
                amount_msats: None,
            },
        );
        assert!(first.is_ok(), "first tap must be accepted");

        let second = module.start(
            &mut ctx(),
            WalletAction::PayInvoice {
                bolt11,
                amount_msats: None,
            },
        );
        match second {
            Err(ActionRejection::Conflict(msg)) => {
                assert!(
                    msg.contains("already in progress"),
                    "conflict message should describe the state: {msg}"
                );
            }
            other => panic!("expected Conflict rejection for duplicate bolt11, got {other:?}"),
        }
    }

    /// Different bolt11 strings are independent — both pass.
    #[test]
    fn different_bolt11_strings_both_pass() {
        let module = WalletPayInvoiceModule::new(handle());

        module
            .start(
                &mut ctx(),
                WalletAction::PayInvoice {
                    bolt11: "lnbc100n1p0aaaa".to_string(),
                    amount_msats: None,
                },
            )
            .expect("first invoice must be accepted");

        module
            .start(
                &mut ctx(),
                WalletAction::PayInvoice {
                    bolt11: "lnbc200n1p0bbbb".to_string(),
                    amount_msats: None,
                },
            )
            .expect("second distinct invoice must be accepted");
    }

    /// After the TTL the same bolt11 is accepted again.
    #[test]
    fn expired_inflight_entry_allows_retry() {
        use std::time::Duration;

        let module = WalletPayInvoiceModule::new(handle());
        let bolt11 = "lnbc500n1p0expired";

        module
            .start(
                &mut ctx(),
                WalletAction::PayInvoice {
                    bolt11: bolt11.to_string(),
                    amount_msats: None,
                },
            )
            .expect("first tap must be accepted");

        // Backdate the entry so it appears expired.
        {
            let mut map = module.inflight_bolt11.get_inner_mut();
            let backdated = std::time::Instant::now()
                .checked_sub(INFLIGHT_BOLT11_TTL + Duration::from_secs(1))
                .expect("Instant::checked_sub(61s) must succeed");
            if let Some(v) = map.get_mut(bolt11) {
                *v = backdated;
            }
        }

        // After expiry the retry passes.
        module
            .start(
                &mut ctx(),
                WalletAction::PayInvoice {
                    bolt11: bolt11.to_string(),
                    amount_msats: None,
                },
            )
            .expect("retry after TTL must be accepted");
    }
}
