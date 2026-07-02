//! `nmp-nip59` — NIP-59 gift-wrap / seal / rumor envelope.
//!
//! # Overview
//!
//! ADR-0072 §D5 reduced this crate to **pure functions** — the `SignerForSeal`
//! trait + driver-thread execution model is deleted. The seal step for a DM send
//! is now a continuation chain through the actor's signer port (composed in
//! `nmp-nip17`); this crate supplies the pure building blocks both that chain and
//! the local-keys callers compose from.
//!
//! Sealing / wrapping (`signer_seal`):
//! - [`build_seal_unsigned`]: assemble the kind:13 seal `UnsignedEvent` from an
//!   already-produced NIP-44 ciphertext (no key material).
//! - [`wrap_signed_seal`]: locally wrap a signed seal in a kind:1059 envelope
//!   under a fresh ephemeral key (NIP-59 unlinkability).
//! - [`gift_wrap_local`]: the local-keys-only convenience composing the four pure
//!   steps end-to-end. For callers local-key-only by construction
//!   (`nmp-marmot`) and the integration tests. Synchronous; no trait, no thread.
//!
//! Unwrapping (`wrap`):
//! - [`unwrap_gift_wrap`]: local-keys kind:1059 → verify seal → extract rumor.
//! - [`parse_outer_for_decrypt`] / [`parse_seal_for_decrypt`] / [`parse_rumor`]:
//!   the three PURE halves the unwrap composes, split out so ADR-0072 Stage 4 can
//!   route the two NIP-44 decrypts through `Nip44DecryptForAccount`.
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

#![cfg_attr(not(any(test, feature = "std")), no_std)]

extern crate alloc;

pub use error::Nip59Error;
pub use kinds::KIND_GIFT_WRAP;
#[cfg(feature = "wrap")]
pub use signer_seal::{build_seal_unsigned, gift_wrap_local, wrap_signed_seal};
pub use wrap::{
    parse_outer_for_decrypt, parse_rumor, parse_seal_for_decrypt, unwrap_gift_wrap, UnwrappedGift,
};

mod error;
pub mod kinds;
#[cfg(feature = "wrap")]
mod signer_seal;
mod wrap;

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
