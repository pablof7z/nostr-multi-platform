//! Cashu mint HTTP client and cryptographic primitives.
//!
//! # Modules
//!
//! - [`crypto`] — DHKE blind signatures and DLEQ proof verification (NUT-00, NUT-12).
//! - [`types`] — HTTP API request/response types (NUT-01 through NUT-12).
//! - [`http`] — pure mint HTTP request construction + response validation
//!   (the "capability lane" — always compiled, no `ureq`; see its module
//!   docs for why native and browser transports both funnel through it).
//! - `client` — Synchronous HTTP client wrapping [`http`] with `ureq`.
//!   Requires the `native` feature; [`http`]'s pure builder/validator
//!   surface stays HTTP-free and always-compiled so a browser transport can
//!   reuse the exact same validation.

#[cfg(feature = "native")]
pub mod client;
pub mod crypto;
pub mod http;
pub mod types;

#[cfg(feature = "native")]
pub use client::MintClient;
pub use crypto::{
    blind_message, hash_to_curve, random_secret, random_secret_hex, unblind_signature, verify_dleq,
    DleqProof,
};
pub use http::{
    build_check_state_request, build_get_keys_request, build_get_keysets_request,
    build_get_mint_quote_bolt11_request, build_mint_quote_bolt11_request,
    finalize_mint_bolt11_response, finalize_swap_response, parse_check_state_response,
    parse_keys_response, parse_mint_quote_bolt11_response, prepare_mint_bolt11_request,
    prepare_swap_request, split_amount, DleqPolicy, MintHttpMethod, MintHttpOperation,
    MintHttpRequest, MintQuoteExpectation, MintRawResponse, PreparedMintBolt11Request,
    PreparedSwapRequest,
};
pub use types::Proof;
