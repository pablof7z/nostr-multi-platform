//! `nmp-nwc` — NIP-47 Nostr Wallet Connect client.
//!
//! Provides URI parsing, NIP-44 encrypted request building, and response
//! decoding for the NWC protocol. The actual actor integration (relay
//! spawning, event routing, kernel snapshot updates) lives in `nmp-core`.
//!
//! ## Usage in `nmp-core`
//!
//! The actor wallet runtime (`actor/commands/wallet.rs`) uses:
//! - [`parse::NwcUri`] to parse a `nostr+walletconnect://` URI
//! - [`build`] functions to build NIP-44 encrypted event content
//! - [`decode::try_decode_response_for_request`] to decode kind:23195
//!   responses and correlate them back to the originating kind:23194 request
//!   id (via the NIP-47 §3.2 `e` tag)
//! - [`crypto::client_pubkey_hex`] to derive the client pubkey from the secret

pub mod build;
pub mod crypto;
pub mod decode;
pub mod kinds;
pub mod parse;
pub mod types;

pub use build::NwcBuildError;
pub use kinds::{KIND_NWC_REQUEST, KIND_NWC_RESPONSE};
pub use parse::{NwcUri, ParseError};
pub use types::{
    GetBalanceResult, GetInfoResult, MakeInvoiceParams, MakeInvoiceResult, NwcError, NwcMethod,
    NwcResponse, PayInvoiceParams, PayInvoiceResult,
};
