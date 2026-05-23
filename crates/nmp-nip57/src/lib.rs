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
//! - **ZapsView** — reactive aggregate (total msats, zappers) keyed by a
//!   zapped event id.

pub mod action;
pub mod bolt11;
pub mod build;
pub mod decode;
pub mod kinds;
pub mod projection;
pub mod view;

pub use action::{ZapAction, ZapInput};
pub use build::{ZapRequest, ZapRequestBuildError, ZapRequestBuilder};
pub use decode::{try_from_event, try_from_kernel_event, ZapReceiptRecord};
pub use kinds::{KIND_ZAP_RECEIPT, KIND_ZAP_REQUEST};
pub use projection::{ZapCount, ZapsAggregateProjection, ZapsAggregateSnapshot};
pub use view::{ZapEntry, ZapsDelta, ZapsPayload, ZapsSpec, ZapsState, ZapsView};

pub fn register_actions(app: &mut nmp_core::NmpApp) {
    app.register_action::<ZapAction>();
}

// `nmp-nip57` exposes `ZapsView` as a plain public type whose `open` /
// `on_event_*` / `snapshot` inherent methods are reached via static dispatch.
// The live extension path is `KernelEventObserver` — see `nmp_core::substrate` docs.
