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
//! - [`WelcomeUnwrapModule`]: DomainModule that ingests kind:1059 gift-wrap
//!   events (`ingest_kinds = &[1059]`).
//!
//! NIP-59 is a generic gift-wrap protocol crate — it deliberately carries no
//! app/protocol nouns (D0). The Marmot Welcome-delivery path consumes the
//! free functions [`gift_wrap`] / [`unwrap_gift_wrap`] directly from
//! `nmp-marmot::service`; there is no Marmot-specific ActionModule here.
//!
//! # Spec
//!
//! <https://github.com/nostr-protocol/nips/blob/master/59.md>

pub mod domain;

pub use domain::WelcomeUnwrapModule;
pub use error::Nip59Error;
pub use wrap::{gift_wrap, unwrap_gift_wrap, UnwrappedGift};

mod error;
mod wrap;

// NOTE: `nmp-nip59` exposes its `DomainModule` impl (`WelcomeUnwrapModule`)
// as a public type. The former `register(&mut ModuleRegistry)` entry point
// was deleted: `ModuleRegistry` only collected name strings and the kernel
// never read them. The live extension path is `KernelEventObserver` — see
// `nmp_core::substrate` docs.
