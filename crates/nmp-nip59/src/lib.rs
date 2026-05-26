//! `nmp-nip59` — NIP-59 gift-wrap / seal / rumor envelope.
//!
//! # Overview
//!
//! This crate provides two load-bearing public free functions:
//! - [`gift_wrap_with_signer`]: ADR-0026 `SignerForSeal` seam — Seal
//!   (kind:13, NIP-44) + gift-wrap (kind:1059, NIP-44 from ephemeral key)
//!   via an abstract signer. The blanket impl on `nostr::Keys` covers the
//!   local-keys fast path (every `SignerOp::Ready`); a remote-signer
//!   adapter (NIP-46 bunker, NIP-07, hardware) drives the chain through a
//!   per-invocation driver thread on the `Pending` path. This is THE
//!   public entry point for both local and remote signers.
//! - [`unwrap_gift_wrap`]: Unwrap an incoming kind:1059 → verify seal →
//!   extract rumor. Thin wrapper over
//!   `nostr::nips::nip59::UnwrappedGift::from_gift_wrap`.
//!
//! The legacy raw-keys primitive `gift_wrap(&Keys, ...)` was tightened to
//! `pub(crate)` in the offline-first audit (PR #631) — every external
//! caller (`nmp-marmot::wrap_welcome`, `nmp-nip17` inbox tests, the
//! integration tests under `tests/`) now routes through
//! `gift_wrap_with_signer`.
//!
//! # D0: no app/protocol nouns
//!
//! NIP-59 is a generic gift-wrap protocol crate — it deliberately carries no
//! app or higher-protocol nouns. Higher-layer consumers (e.g. the MLS
//! group-messaging crate's Welcome-delivery path) call the public free
//! functions directly; each such consumer owns its own kind:1059 ingest
//! path and record shape. There is no MLS/Welcome-aware projection here.
//!
//! # Spec
//!
//! <https://github.com/nostr-protocol/nips/blob/master/59.md>

pub use error::Nip59Error;
pub use kinds::KIND_GIFT_WRAP;
pub use signer_seal::{
    gift_wrap_with_signer, SignerForSeal, DRIVER_STEP_TIMEOUT, GIFT_WRAP_TOTAL_TIMEOUT,
};
pub use wrap::{unwrap_gift_wrap, UnwrappedGift};

mod error;
pub mod kinds;
mod signer_seal;
mod wrap;
