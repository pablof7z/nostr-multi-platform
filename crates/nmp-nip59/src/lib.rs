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
//! `KeyringCapability` surface) is deferred to a post-v1 milestone.
//!
//! # D0: no app/protocol nouns
//!
//! NIP-59 is a generic gift-wrap protocol crate — it deliberately carries no
//! app or higher-protocol nouns. Higher-layer consumers (e.g. the MLS
//! group-messaging crate's Welcome-delivery path) call the free functions
//! [`gift_wrap`] / [`unwrap_gift_wrap`] directly; each such consumer owns
//! its own kind:1059 ingest path and record shape. There is no
//! MLS/Welcome-aware projection here.
//!
//! # Spec
//!
//! <https://github.com/nostr-protocol/nips/blob/master/59.md>

pub use error::Nip59Error;
pub use signer_seal::{
    gift_wrap_with_signer, SignerForSeal, DRIVER_STEP_TIMEOUT, GIFT_WRAP_TOTAL_TIMEOUT,
};
pub use wrap::{gift_wrap, unwrap_gift_wrap, UnwrappedGift};

mod error;
mod signer_seal;
mod wrap;
