//! `nmp-nip57` — NIP-57 lightning zaps as an NMP protocol crate.
//!
//! Implements the design recommendation in `docs/design/kind-wrappers.md` §3
//! restricted to the parts the client owns:
//!
//! - **Zap request** (kind:9734) — client-built. Surface: [`ZapRequest`] +
//!   [`ZapRequestBuilder`] producing an `UnsignedEvent`.
//! - **Zap receipt** (kind:9735) — LN-provider-minted; the client never
//!   builds one. Surface: [`ZapReceiptRecord`] + [`try_from_event`] decoder,
//!   plus a minimal [`bolt11::amount_msats`] HRP parser for the
//!   authoritative payment amount.
//! - **`ZapsView`** — reactive aggregate (total msats, zappers) keyed by a
//!   zapped event id.

pub mod action;
pub mod bolt11;
pub mod build;
pub mod decode;
pub mod interests;
pub mod kinds;
// LNURL-pay HTTP worker: uses `ureq` + `std::thread::spawn` — native only.
#[cfg(feature = "native")]
pub mod lnurl;
pub mod pending;
pub mod projection;
pub mod view;
pub mod wire;

pub use action::{ZapAction, ZapInput};
pub use build::{ZapRequest, ZapRequestBuildError, ZapRequestBuilder};
pub use decode::{try_from_event, try_from_kernel_event, ZapReceiptRecord};
pub use interests::{
    self_zap_receipts_identity, self_zap_receipts_interest, self_zap_receipts_interest_id,
};
pub use kinds::{KIND_ZAP_RECEIPT, KIND_ZAP_REQUEST};
#[cfg(feature = "native")]
pub use lnurl::{sign_zap_request, FetchLnurlInvoiceCommand, LnurlInvoice};
pub use pending::{
    active_pending_zap_registry, new_pending_zap_registry, try_from_kernel_event_validated,
    PendingZapRegistry, PendingZapRegistryHandle, ZapReceiptProviderMismatch,
};
pub use projection::{ZapCount, ZapsAggregateProjection, ZapsAggregateSnapshot};
pub use view::{ZapEntry, ZapsDelta, ZapsPayload, ZapsSpec, ZapsState, ZapsView};
pub use wire::typed_fb::{
    decode_zaps_snapshot, encode_zaps_snapshot, FILE_IDENTIFIER as ZAPS_FILE_IDENTIFIER,
    SCHEMA_ID as ZAPS_SCHEMA_ID, SCHEMA_VERSION as ZAPS_SCHEMA_VERSION,
};

/// Register the NIP-57 zap action module as a yielding default, with NO payment
/// port wired (ADR-0049 Part 1: an app may pre-empt it regardless of call
/// order).
///
/// ADR-0052 rung 5.2: the arity is STABLE across the `native` feature (cargo
/// feature unification flips `native` on globally for any consumer that
/// enables it; a feature-dependent arity would break this call site). The zap
/// auto-chain reaches the wallet through the per-app `PaymentPort` the
/// `ZapAction` value owns — `None` here. A wallet-capable composition root
/// replaces this default with a port-carrying value via
/// [`register_zap_with_payment_port`].
pub fn register_actions(app: &mut impl nmp_core::substrate::ActionRegistrar) {
    app.register_default_action(ZapAction::new());
}

/// Register the NIP-57 zap action module bound to a per-app [`PaymentPort`],
/// via the **app path** (overriding any prior yielding default — ADR-0049: an
/// app replacing a default is legal and order-independent).
///
/// ADR-0052 rung 5.2: the composition root injects a payment port backed by the
/// SAME wallet it wires into the rest of the app (e.g.
/// `nmp_nip47::wallet_payment_port(handle)`), so the zap → LNURL-pay →
/// pay-invoice auto-chain pays through THIS app's wallet (no process-global).
/// `native` only — the LNURL-pay → pay-invoice chain requires the `native`
/// HTTP worker.
#[cfg(feature = "native")]
pub fn register_zap_with_payment_port(
    app: &mut impl nmp_core::substrate::ActionRegistrar,
    payment_port: std::sync::Arc<dyn nmp_core::substrate::PaymentPort>,
) {
    app.register_action(ZapAction::with_payment_port(payment_port))
        .expect("duplicate registration: nmp-nip57 ZapAction"); // doctrine-allow: D6 — startup-only call; RegistrationError here is a programmer error (duplicate wiring), not a runtime failure
}

// `nmp-nip57` exposes `ZapsView` as a plain public type whose `open` /
// `on_event_*` / `snapshot` inherent methods are reached via static dispatch.
// The live extension path is `KernelEventObserver` — see `nmp_core::substrate` docs.
