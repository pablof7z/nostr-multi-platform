//! `nmp.nip57.zap` — the NIP-57 lightning zap [`ActionModule`].
//!
//! Validates a zap request in [`ZapAction::start`], builds an unsigned
//! kind:9734 via [`ZapRequest`] (`crate::build`), and dispatches
//! [`ActorCommand::Protocol`] carrying a
//! [`crate::lnurl::FetchLnurlInvoiceCommand`] (V-41 — the LNURL-pay
//! round-trip is now a `ProtocolCommand`; the legacy `FetchLnurlInvoice`
//! `ActorCommand` variant has been deleted along with the
//! `nmp-core::actor::commands::zap` module). The protocol command signs
//! the kind:9734 on the actor thread, fetches the receiver's LNURL
//! callback off-thread, and surfaces the resulting bolt11 invoice as a
//! `ShowToast` follow-up.
//!
//! # Wire routing
//!
//! NIP-57 § "Appendix C": the signed kind:9734 goes to the LN provider's
//! LNURL **callback URL** as `nostr=<urlencoded>` — NOT to Nostr relays.
//! The kind:9735 receipt is what relays receive; the LN provider mints it
//! after the invoice settles.
//!
//! # Signing (V-78 — bunker zaps, reconciled onto the unified port)
//!
//! The protocol command signs the kind:9734 through the unified
//! [`ActorCommand::SignEventForAccount`] port via
//! [`nmp_core::substrate::ProtocolCommandContext::sign_event_for_account`]
//! (ADR-0043 Decision 2). The actor's dispatch arm resolves BOTH signer kinds
//! behind the port — a local nsec signs inline (`SignerOp::Ready`); a NIP-46
//! bunker parks (`SignerOp::Pending`) and the idle-loop drain resolves it —
//! then invokes the command's continuation with the resolved `SignedEvent`. The
//! continuation spawns the off-actor LNURL HTTP worker (D8 — never the actor
//! loop) carrying the already-signed event; it never branches on backend. Only
//! a genuinely absent account (no local key AND no remote signer) surfaces an
//! `Err` to the continuation, which fails closed with a toast +
//! `RecordActionFailure`. One signing seam, both backends (D13 — only a
//! `SignedEvent` ever crosses the port).

use nmp_core::substrate::{ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRejection};
use nmp_core::actor::ActorCommand;
use serde::{Deserialize, Serialize};

use crate::build::ZapRequest;
#[cfg(feature = "native")]
use crate::lnurl::FetchLnurlInvoiceCommand;
#[cfg(feature = "native")]
use std::sync::Arc;
#[cfg(feature = "native")]
use nmp_core::substrate::PaymentPort;

/// Wire shape for `nmp.nip57.zap` — the JSON body a host passes to
/// `nmp_app_dispatch_action`.
///
/// ```json
/// {
///   "recipient_pubkey": "<hex>",
///   "amount_msats": 21000,
///   "target_event_id": "<hex>",
///   "comment": "🤙"
/// }
/// ```
///
/// `lnurl` is optional. When omitted the kernel resolves the recipient's
/// lightning address from its cached kind:0 profile and fails with a clear
/// toast if none is found. Shells SHOULD omit `lnurl` — it is a protocol
/// detail the kernel owns, not the app. When provided (e.g. by the `:zap`
/// power-user command that lets the caller override the destination) it is
/// used verbatim.
///
/// `relays` is optional (`[]` or omitted) — the actor injects via the
/// substrate `RecipientRelayLookup` capability (kernel-side adapter
/// routes through `outbox_router` with a synthetic kind:9735 publish to
/// resolve the recipient's NIP-65 write set) before signing (V-07).
///
/// `target_event_id` and `comment` are optional. A zap to a profile (no
/// target event) omits `target_event_id`. `relays` may be empty, in which
/// case the actor selects from the recipient's kind:10002 (NIP-65) write
/// relays before signing — that's the only D0-correct answer (V-07).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ZapInput {
    /// Recipient's Nostr public key, lowercase hex.
    pub recipient_pubkey: String,
    /// Amount in millisatoshis. Must be > 0.
    pub amount_msats: u64,
    /// Receiver's LNURL-pay endpoint — lightning address, bech32 LNURL, or
    /// bare https URL. When `None` the kernel resolves it from the
    /// recipient's cached kind:0 profile (`lud16` / `lud06`). Shells
    /// SHOULD omit this field; it is a protocol detail.
    #[serde(default)]
    pub lnurl: Option<String>,
    /// Relay URLs for the kind:9734 `relays` tag. When empty the actor
    /// auto-selects from the recipient's kind:10002 (NIP-65) write/both
    /// relays — relay selection is policy that lives in the kernel, never
    /// the shell (V-07).
    #[serde(default)]
    pub relays: Vec<String>,
    /// Optional zapped event id (hex). When set, the kind:9734 carries an `e`
    /// tag pointing at the target note.
    #[serde(default)]
    pub target_event_id: Option<String>,
    /// Optional free-form comment — becomes the kind:9734 `content`.
    #[serde(default)]
    pub comment: Option<String>,
}

/// The `nmp.nip57.zap` [`ActionModule`].
///
/// `start` validates the zap input. `execute` builds the unsigned
/// kind:9734 zap request via [`ZapRequestBuilder`] and enqueues
/// [`ActorCommand::Protocol`] carrying a
/// [`FetchLnurlInvoiceCommand`] (V-41) — the protocol command handles
/// signing (D7 — kernel owns key access) and the off-thread LNURL-pay
/// HTTP round-trip (D8 — no blocking on the actor thread).
///
/// ADR-0052 rung 5.2: under the `native` feature the module owns an OPTIONAL
/// per-app [`PaymentPort`] so the zap → pay_invoice auto-chain pays through
/// THIS app's wallet (captured at composition time), not a process-global. The
/// port is the substrate seam (`nmp_core::substrate::PaymentPort`); NIP-57 no
/// longer names a NIP-47 wallet runtime — `nmp-nip47` supplies the concrete
/// adapter (`WalletPaymentPort`) and composition injects it. The port is cloned
/// into each [`FetchLnurlInvoiceCommand`] `execute` produces.
///
/// `None` means no wallet was wired (a host that registered the zap default
/// but never composed a payment port). The auto-chain then records a clear
/// "no wallet connected" action failure. A wallet-capable composition root
/// replaces the `None` default with a `Some(port)` value via
/// [`crate::register_zap_with_payment_port`] (the app-path override of the
/// yielding default — ADR-0049).
///
/// The port is `Option<…>` (not arity-split constructors) so
/// `register_actions(app)` keeps a STABLE arity across the `native` feature —
/// cargo feature unification flips `native` on globally when any consumer
/// enables it, and a feature-dependent arity would break the non-wallet call
/// sites (`nmp-defaults`, tests).
#[derive(Default)]
pub struct ZapAction {
    #[cfg(feature = "native")]
    payment_port: Option<Arc<dyn PaymentPort>>,
}

impl ZapAction {
    /// Construct the zap module with NO payment port (the yielding default).
    /// The auto-chain records a "no wallet connected" failure until a
    /// wallet-capable root overrides it via
    /// [`crate::register_zap_with_payment_port`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct the zap module bound to a per-app [`PaymentPort`] (`native`
    /// builds — the only ones with the LNURL-pay → pay-invoice chain).
    #[cfg(feature = "native")]
    #[must_use]
    pub fn with_payment_port(payment_port: Arc<dyn PaymentPort>) -> Self {
        Self {
            payment_port: Some(payment_port),
        }
    }
}

fn record_action_failure(send: &dyn Fn(ActorCommand), correlation_id: &str, reason: String) {
    send(ActorCommand::RecordActionFailure {
        correlation_id: correlation_id.to_string(),
        reason,
    });
}

impl ActionModule for ZapAction {
    const NAMESPACE: &'static str = "nmp.nip57.zap";
    type Action = ZapInput;

    /// ADR-0064 / S9: opt into the typed FlatBuffers payload doorway; the
    /// fail-closed `schema_version` gate runs in `decode` (BEFORE `start`).
    fn decode_payload(
        bytes: &[u8],
    ) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<ZapInput as ActionPayload>::decode(bytes))
    }

    /// Validate a zap request. Rejects:
    /// - empty `recipient_pubkey`
    /// - `amount_msats == 0`
    ///
    /// `lnurl` may be omitted — the kernel resolves it from the recipient's
    /// cached kind:0 profile at execute time. `relays` may be empty: the
    /// actor auto-selects from the recipient's kind:10002 (NIP-65) write
    /// list before signing (V-07). Relay choice is policy that lives in the
    /// kernel, not the shell.
    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        if action.recipient_pubkey.trim().is_empty() {
            return Err(ActionRejection::Invalid(
                "zap requires a recipient pubkey".into(),
            ));
        }
        if action.amount_msats == 0 {
            return Err(ActionRejection::Invalid(
                "zap amount must be greater than 0 msats".into(),
            ));
        }
        if action
            .lnurl
            .as_deref()
            .is_some_and(|lnurl| lnurl.trim().is_empty())
        {
            return Err(ActionRejection::Invalid(
                "zap lnurl must not be empty when provided".into(),
            ));
        }
        Ok(())
    }

    /// Settles asynchronously: `execute` enqueues
    /// `Protocol(FetchLnurlInvoiceCommand{...})` and returns immediately;
    /// the HTTP worker spawned in `FetchLnurlInvoiceCommand::run` dispatches
    /// NWC pay-invoice on a fetched invoice, or records `Failed` on LNURL /
    /// missing-wallet errors. The NIP-47 kind:23195 response closes the
    /// original zap action's `correlation_id` on wallet confirmation.
    fn is_async_completing() -> bool {
        true
    }

    /// Build the unsigned kind:9734 and enqueue an
    /// [`ActorCommand::Protocol`] carrying a
    /// [`FetchLnurlInvoiceCommand`] (V-41).
    ///
    /// # D7 — kernel owns the wall clock
    ///
    /// `created_at` is passed as `0`; the protocol command re-stamps from
    /// `ProtocolCommandContext::now_secs` before signing. Matches the
    /// `PublishUnsignedEventToRelays` precedent.
    ///
    /// # D8 — no blocking
    ///
    /// The closure neither HTTPs nor signs; the LNURL command's `run`
    /// does both: the kind:9734 signature on the actor thread (D7), the
    /// LNURL-pay HTTP round-trip on a spawned `std::thread::spawn`
    /// worker (D8).
    fn execute(
        &self,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        // Filter empty/whitespace relays (already partially done in start;
        // re-filter so the builder gets the cleaned set without re-running
        // the validator).
        let relays: Vec<String> = action
            .relays
            .iter()
            .filter(|r| !r.trim().is_empty())
            .cloned()
            .collect();
        let mut builder = ZapRequest::to_pubkey(&action.recipient_pubkey)
            .amount_msats(action.amount_msats)
            .relays(relays);
        if let Some(ref id) = action.target_event_id {
            builder = builder.zapped_event(id);
        }
        if let Some(ref comment) = action.comment {
            builder = builder.comment(comment);
        }
        // D7: `pubkey` and `created_at` are sentinels set internally by the
        // builder — the protocol command re-stamps both from the active Keys
        // and `ctx.now_secs()` in `FetchLnurlInvoiceCommand::run`.
        let unsigned = match builder.build() {
            Ok(unsigned) => unsigned,
            Err(e) => {
                record_action_failure(
                    send,
                    correlation_id,
                    format!("build kind:9734 zap request: {e}"),
                );
                return Ok(());
            }
        };
        #[cfg(feature = "native")]
        send(ActorCommand::Protocol(Box::new(FetchLnurlInvoiceCommand {
            unsigned,
            recipient_pubkey: action.recipient_pubkey,
            lnurl_or_address: action.lnurl,
            amount_msats: action.amount_msats,
            correlation_id: Some(correlation_id.to_string()),
            payment_port: self.payment_port.clone(),
        })));
        // NOTE: `self.payment_port` is `Option<Arc<dyn PaymentPort>>`; a `None`
        // surfaces as a "no wallet connected" failure inside the worker.
        #[cfg(not(feature = "native"))]
        { let _ = (unsigned, action); record_action_failure(send, correlation_id, "zap not available on this platform".into()); }
        Ok(())
    }
}

#[cfg(test)]
#[path = "action_tests.rs"]
mod tests;
