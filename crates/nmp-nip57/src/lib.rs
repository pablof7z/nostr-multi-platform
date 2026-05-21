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
//! - **ZapsDomain** — `(zapped_event_id → receipt_ids)` reverse-index.

pub mod action;
pub mod bolt11;
pub mod build;
pub mod decode;
pub mod domain;
pub mod kinds;
pub mod view;

pub use action::{zap_request_command, ZapAction, ZapModule};
pub use build::{ZapRequest, ZapRequestBuildError, ZapRequestBuilder};
pub use decode::{try_from_event, try_from_kernel_event, ZapReceiptRecord};
pub use domain::{decode_and_route, list_by_target, ZapsDomain, NAMESPACE};
pub use kinds::{KIND_ZAP_RECEIPT, KIND_ZAP_REQUEST};
pub use view::{ZapEntry, ZapsDelta, ZapsPayload, ZapsSpec, ZapsState, ZapsView};

// NOTE: `nmp-nip57` exposes its `DomainModule` impl and `ZapsView` type
// (`ZapsDomain`, `ZapsView`) as public types. `ZapsView` is a plain type
// whose `open` / `on_event_*` / `snapshot` inherent methods are reached via
// static dispatch — the `ViewModule` trait and the former
// `register(&mut ModuleRegistry)` entry point were both deleted because no
// kernel-side registry ever drove them. The live extension path is
// `KernelEventObserver` — see `nmp_core::substrate` docs.
