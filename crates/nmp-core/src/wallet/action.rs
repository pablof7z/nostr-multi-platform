//! NIP-47 wallet `ActionModule`s — the `nmp.wallet.*` namespaces routed
//! through `nmp_app_dispatch_action`.
//!
//! # Why this exists (V3 — `dispatch_action` is the sole user-write seam)
//!
//! Per D4, every user-initiated write enters the kernel through
//! `nmp_app_dispatch_action`. Before this module, the C-ABI symbol
//! `nmp_app_wallet_pay_invoice` (see [`crate::ffi::wallet`]) bypassed that
//! seam by constructing [`crate::actor::ActorCommand::WalletPayInvoice`]
//! directly — the V3 violation flagged in `docs/architecture-audit/`.
//!
//! Adding [`WalletPayInvoiceModule`] under namespace `nmp.wallet.pay_invoice`
//! closes the bypass: the bespoke FFI now translates its arguments into a
//! [`WalletAction::PayInvoice`] payload and routes the call through
//! [`crate::ffi::action::nmp_app_dispatch_action`]. Every accepted dispatch
//! mints a `correlation_id` and reaches
//! [`crate::actor::commands::wallet::wallet_pay_invoice`] through the
//! registry's `execute()` path — the same single entry point a future caller
//! that calls `dispatch_action` directly would use.
//!
//! # Theme A discriminator (D4, [`crate::substrate::action`])
//!
//! NIP-47 NWC is connection-oriented protocol glue, NOT a content action —
//! the wallet does NOT author or publish Nostr events on the user's
//! behalf. Theme A names the wallet as one of the "system-authored /
//! lifecycle / wallet capabilities" that traditionally stayed on bespoke FFI
//! symbols. The V3 cut promotes only the user-initiated *intent* surface
//! (`pay_invoice` — "the user wants to pay this invoice") onto the action
//! seam; connection-management (`wallet_connect` / `wallet_disconnect`)
//! stays bespoke because it addresses an in-process connection lifecycle, not
//! a user-authored action.
//!
//! # Async completion (D12)
//!
//! `pay_invoice` settles asynchronously: the actor signs and sends the
//! kind:23194 request, and the eventual outcome arrives later as a
//! kind:23195 response handled in [`crate::actor::commands::wallet`].
//! [`WalletPayInvoiceModule::is_async_completing`] therefore returns `true`
//! so the host sees a lifecycle through `projections["action_stages"]`.
//! Stage recording happens in `actor/commands/wallet.rs`
//! (`record_action_success` / `record_action_failure` call sites) — cross-
//! file from this declaration, hence the `doctrine-allow: D12` annotation on
//! the marker itself.

use serde::{Deserialize, Serialize};

use crate::actor::ActorCommand;
use crate::substrate::{ActionContext, ActionModule, ActionRejection};

/// User-initiated wallet intents dispatchable through
/// [`crate::ffi::action::nmp_app_dispatch_action`] under the
/// `nmp.wallet.pay_invoice` namespace.
///
/// `PayInvoice` is currently the only variant: connection lifecycle
/// (`wallet_connect` / `wallet_disconnect`) stays on dedicated FFI symbols
/// per the Theme A discriminator (see module docs).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum WalletAction {
    /// Pay a BOLT-11 Lightning invoice via the connected NIP-47 wallet.
    ///
    /// `bolt11`: the invoice string the user wants to pay.
    /// `amount_msats`: optional override for invoices that omit an embedded
    /// amount; `None` means "use the invoice's embedded amount".
    PayInvoice {
        bolt11: String,
        amount_msats: Option<u64>,
    },
}

/// `ActionModule` implementation for `nmp.wallet.pay_invoice`.
///
/// Stateless ZST adapter — the actor side
/// ([`crate::actor::commands::wallet::wallet_pay_invoice`]) owns the
/// connection state, double-tap-against-the-wire dedup, and async response
/// matching via `pending_payments`.
pub struct WalletPayInvoiceModule;

impl ActionModule for WalletPayInvoiceModule {
    const NAMESPACE: &'static str = "nmp.wallet.pay_invoice";

    type Action = WalletAction;

    /// Validate the action shape. `bolt11` must be non-empty — an empty
    /// invoice is a caller bug, not "use the embedded amount", and would
    /// produce an unparseable NWC request downstream.
    ///
    /// Per-invoice / per-amount semantic validation (BOLT-11 well-formedness,
    /// amount sanity) is the wallet's job at sign / encode time; the action
    /// gate is "is this a syntactically dispatchable intent?". This mirrors
    /// the `PublishAction::PublishNote` gate (non-empty content) — the
    /// engine validates the rest.
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

    /// `pay_invoice` settles asynchronously — the actor signs+sends the
    /// kind:23194 NWC request, and the eventual outcome arrives later as a
    /// kind:23195 response handled in `actor/commands/wallet.rs`. Recording
    /// sites for the resulting `Accepted` / `Failed` stages are cross-file
    /// (`actor/commands/wallet.rs`'s `record_action_success` /
    /// `record_action_failure` calls), the same shape `PublishModule` has.
    /// Cross-file recording cannot be statically verified per D12 (the rule
    /// is grep-level per-file), so the marker carries an explicit allow.
    fn is_async_completing() -> bool { // doctrine-allow: D12 — recording sites are cross-file (actor/commands/wallet.rs `record_action_success`/`record_action_failure`); covered by `wallet.rs` runtime tests
        true
    }

    /// Translate the validated action into the existing
    /// [`ActorCommand::WalletPayInvoice`] enqueued onto the actor channel.
    ///
    /// `correlation_id` is the registry-minted action id — threaded onto the
    /// command so the wallet runtime can stash
    /// `kind23194_event_id → correlation_id` in its `pending_payments` map
    /// and route the kind:23195 outcome back to
    /// [`crate::kernel::Kernel::record_action_success`] /
    /// [`crate::kernel::Kernel::record_action_failure`] (closing the host's
    /// spinner round-trip).
    fn execute(
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        match action {
            WalletAction::PayInvoice { bolt11, amount_msats } => {
                send(ActorCommand::WalletPayInvoice {
                    bolt11,
                    amount_msats,
                    correlation_id: Some(correlation_id.to_string()),
                });
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

    /// A well-formed `PayInvoice` passes `start`. The validator does not
    /// inspect bolt11 semantics — only that it is non-empty — so a fake
    /// invoice string is sufficient to exercise the gate (the wallet runtime
    /// is responsible for BOLT-11 parsing on the wire side).
    #[test]
    fn start_accepts_non_empty_bolt11() {
        let action = WalletAction::PayInvoice {
            bolt11: "lnbc100n1p0fakeinvoice".to_string(),
            amount_msats: None,
        };
        WalletPayInvoiceModule::start(&mut ctx(), action)
            .expect("non-empty bolt11 must be accepted");
    }

    /// `start` accepts an `amount_msats` override — the host can pay a
    /// zero-amount invoice (LUD-06 / amountless) by passing the desired
    /// amount alongside the bolt11.
    #[test]
    fn start_accepts_explicit_amount_msats() {
        let action = WalletAction::PayInvoice {
            bolt11: "lnbc1p0amountless".to_string(),
            amount_msats: Some(21_000),
        };
        WalletPayInvoiceModule::start(&mut ctx(), action)
            .expect("explicit amount must be accepted");
    }

    /// An empty `bolt11` is rejected as `Invalid`. The downstream NWC
    /// request build would fail with an opaque encoding error; failing
    /// closed at `start` surfaces a useful message to the host before any
    /// `ActorCommand` is enqueued.
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

    /// `is_async_completing` MUST return `true` so the host's
    /// `projections["action_stages"]` lifecycle covers the pay_invoice
    /// round-trip. Locked in via this test so a future contributor cannot
    /// silently flip it to `false` (which would orphan the action's stage
    /// mirror).
    #[test]
    fn is_async_completing_is_true() {
        assert!(
            WalletPayInvoiceModule::is_async_completing(),
            "pay_invoice settles asynchronously via the kind:23195 response"
        );
    }

    /// The executor builds exactly one [`ActorCommand::WalletPayInvoice`]
    /// carrying the registry-minted `correlation_id`. The wallet runtime
    /// uses this id to close the round-trip through
    /// `Kernel::record_action_success` / `record_action_failure` on the
    /// matching kind:23195 response.
    #[test]
    fn execute_emits_wallet_pay_invoice_with_correlation_id() {
        use std::cell::RefCell;

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
        assert_eq!(cmds.len(), 1, "executor must emit exactly one ActorCommand; got {cmds:?}");
        match cmds.into_iter().next().unwrap() {
            ActorCommand::WalletPayInvoice {
                bolt11,
                amount_msats,
                correlation_id,
            } => {
                assert_eq!(bolt11, "lnbc500n1p0testinvoice");
                assert_eq!(amount_msats, Some(1_234));
                assert_eq!(
                    correlation_id,
                    Some(minted_correlation_id),
                    "executor must thread the registry-minted correlation_id onto the command"
                );
            }
            other => panic!("expected ActorCommand::WalletPayInvoice, got {other:?}"),
        }
    }

    /// A `None` `amount_msats` (use the embedded amount) is preserved
    /// through `execute` — the wallet runtime distinguishes "embedded
    /// amount" from "override" by checking `amount_msats.is_some()`, so
    /// silently mapping `None → Some(0)` would change the wire behaviour.
    #[test]
    fn execute_preserves_none_amount_msats() {
        use std::cell::RefCell;

        let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
        let action = WalletAction::PayInvoice {
            bolt11: "lnbc100n1p0embedded".to_string(),
            amount_msats: None,
        };

        WalletPayInvoiceModule::execute(action, "cid", &|cmd| {
            captured.borrow_mut().push(cmd);
        })
        .expect("execute must succeed");

        match captured.into_inner().into_iter().next().unwrap() {
            ActorCommand::WalletPayInvoice { amount_msats, .. } => {
                assert_eq!(
                    amount_msats, None,
                    "None must be preserved verbatim — the wallet runtime uses Some/None as the embedded-vs-override switch"
                );
            }
            other => panic!("expected ActorCommand::WalletPayInvoice, got {other:?}"),
        }
    }

    /// `WalletAction` round-trips through `serde_json::to_string` /
    /// `from_str`. The action seam carries action JSON across the FFI
    /// boundary (and a thin-wrapper FFI symbol like
    /// `nmp_app_wallet_pay_invoice` synthesises this JSON on the call
    /// side), so the serde shape must be stable. Locks in the externally-
    /// tagged enum representation (`{"PayInvoice":{...}}`) as the wire
    /// shape any caller — Rust, Swift, Kotlin — must produce.
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
}
