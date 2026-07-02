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
//! - **`ZapsView`** — scoped, single-target receipt view keyed by a zapped
//!   event id.

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
pub mod ui_codes;
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
pub use view::{ZapEntry, ZapsDelta, ZapsPayload, ZapsSpec, ZapsState, ZapsView};

#[derive(Clone, Default)]
pub struct Config {
    #[cfg(feature = "native")]
    payment_port: Option<std::sync::Arc<dyn nmp_core::substrate::PaymentPort>>,
}

#[cfg(feature = "native")]
impl Config {
    #[must_use]
    pub fn with_payment_port(
        payment_port: std::sync::Arc<dyn nmp_core::substrate::PaymentPort>,
    ) -> Self {
        Self {
            payment_port: Some(payment_port),
        }
    }
}

impl core::fmt::Debug for Config {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut debug = f.debug_struct("Config");
        #[cfg(feature = "native")]
        debug.field("payment_port", &self.payment_port.is_some());
        debug.finish()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Handles {}

/// Register the NIP-57 zap action module.
///
/// Without a payment port, the module is a yielding default (ADR-0069 Part 1).
/// With a payment port, the module is installed on the app path so a
/// wallet-capable composition root can override the default and route zap
/// payment through its own wallet runtime.
pub fn register(
    app: &mut impl nmp_core::substrate::ActionRegistrar,
    _config: Config,
) -> Result<Handles, nmp_core::substrate::RegistrationError> {
    #[cfg(feature = "native")]
    if let Some(payment_port) = _config.payment_port {
        app.register_action(ZapAction::with_payment_port(payment_port))?;
        return Ok(Handles {});
    }

    app.register_default_action(ZapAction::new());
    Ok(Handles {})
}

// `nmp-nip57` exposes `ZapsView` as a plain public type whose `open` /
// `on_event_*` / `snapshot` inherent methods are reached via static dispatch.
// Timeline/card zap counts should mount a zap-owned read or app-owned social
// bar recipe. There is no central relation aggregator.

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
