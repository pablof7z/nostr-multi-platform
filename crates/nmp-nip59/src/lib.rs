//! `nmp-nip59` — NIP-59 gift-wrap / seal / rumor envelope.
//!
//! # Overview
//!
//! This crate provides two load-bearing free functions:
//! - [`gift_wrap`]: Seal (kind:13, NIP-44) + gift-wrap (kind:1059, NIP-44 from
//!   ephemeral key). Thin wrapper over `nostr::EventBuilder::gift_wrap`.
//! - [`unwrap_gift_wrap`]: Unwrap an incoming kind:1059 → verify seal → extract
//!   rumor. Thin wrapper over `nostr::nips::nip59::UnwrappedGift::from_gift_wrap`.
//!
//! Both functions operate at the `nostr::Keys` boundary — the caller supplies
//! real key material. This is the appropriate seam for the M11.5 milestone;
//! full kernel signer-bridge wiring (where the actor fetches keys via the
//! `KeyringCapability` surface) is deferred to the post-v1 Marmot milestone.
//!
//! # Substrate modules
//!
//! - [`WelcomeWrapModule`]: ActionModule that takes an MLS Welcome rumor
//!   (`UnsignedEvent`) and emits a [`WrapPlan`] carrier for routing.
//! - [`WelcomeUnwrapModule`]: DomainModule that ingests kind:1059 gift-wrap
//!   events (`ingest_kinds = &[1059]`).
//!
//! # Seam documentation
//!
//! `WelcomeWrapModule::start` emits a [`WrapPlan`] rather than a bare
//! `PublishPlan { kind, content, tags }`. The gift-wrap operation requires
//! the sender's `Keys` to NIP-44 encrypt the seal. The NMP substrate
//! ActionModule interface does not currently thread live key material through
//! `ActionContext`. The resolution path:
//! - Short term (this milestone): callers invoke the free function
//!   [`gift_wrap`] directly when they hold keys.
//! - Long term: the actor's signer-bridge will recognise `WrapPlan` in its
//!   `AwaitCapability` step and call `gift_wrap` on the actor side where the
//!   `KeyringCapability` is available.
//!
//! # Spec
//!
//! <https://github.com/nostr-protocol/nips/blob/master/59.md>

pub mod action;
pub mod domain;

pub use action::WelcomeWrapModule;
pub use domain::WelcomeUnwrapModule;
pub use error::Nip59Error;
pub use wrap::{gift_wrap, unwrap_gift_wrap, UnwrappedGift};

mod error;
mod wrap;

// NOTE: `nmp-nip59` exposes its `ActionModule` / `DomainModule` impls
// (`WelcomeWrapModule`, `WelcomeUnwrapModule`) as public types. The former
// `register(&mut ModuleRegistry)` entry point was deleted: `ModuleRegistry`
// only collected name strings and the kernel never read them. The live
// extension path is `KernelEventObserver` — see `nmp_core::substrate` docs.
