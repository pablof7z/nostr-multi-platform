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
pub mod projection;
pub mod view;
pub mod wire;

pub use action::{ZapAction, ZapInput};
pub use build::{ZapRequest, ZapRequestBuildError, ZapRequestBuilder};
pub use decode::{try_from_event, try_from_kernel_event, ZapReceiptRecord};
pub use interests::{self_zap_receipts_interest, self_zap_receipts_interest_id};
pub use kinds::{KIND_ZAP_RECEIPT, KIND_ZAP_REQUEST};
#[cfg(feature = "native")]
pub use lnurl::{sign_zap_request, FetchLnurlInvoiceCommand};
pub use projection::{ZapCount, ZapsAggregateProjection, ZapsAggregateSnapshot};
pub use view::{ZapEntry, ZapsDelta, ZapsPayload, ZapsSpec, ZapsState, ZapsView};
pub use wire::typed_fb::{
    decode_zaps_snapshot, encode_zaps_snapshot, FILE_IDENTIFIER as ZAPS_FILE_IDENTIFIER,
    SCHEMA_ID as ZAPS_SCHEMA_ID, SCHEMA_VERSION as ZAPS_SCHEMA_VERSION,
};

pub fn register_actions(app: &mut impl nmp_core::substrate::ActionRegistrar) {
    // Yielding default (ADR-0049 Part 1): an app may pre-empt the zap action
    // module regardless of call order.
    app.register_default_action::<ZapAction>();
}

// `nmp-nip57` exposes `ZapsView` as a plain public type whose `open` /
// `on_event_*` / `snapshot` inherent methods are reached via static dispatch.
// The live extension path is `KernelEventObserver` — see `nmp_core::substrate` docs.
