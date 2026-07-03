//! Cashu HTTP API request/response types (NUT-01 through NUT-12).
//!
//! # Redacted `Debug`
//!
//! Several types here carry money-adjacent secrets — a proof's spending
//! `secret`, a DLEQ blinding factor `r`, a mint quote id (ties a wallet to a
//! specific pending payment), or a Lightning payment preimage. Those types
//! implement `Debug` by hand instead of deriving it, printing `"<redacted>"`
//! for the sensitive field(s) so a stray `{:?}` in a log line or panic
//! message cannot leak them. See each submodule's `sensitive_debug_redacts_*`
//! tests.
//!
//! # Module layout (file-size discipline)
//!
//! - [`mint_quote`] — NUT-04 mint-quote request/response.
//! - [`minting`] — NUT-04 mint-tokens request/response + NUT-12 DLEQ wire type.
//! - [`proof`] — the [`Proof`] type + NUT-03 swap request/response.
//! - [`state`] — NUT-07 proof state check.
//! - [`melt`] — NUT-05 melt request/response (not wired to any wallet flow
//!   yet — shapes + validation groundwork only, per epic #2864).

mod melt;
mod mint_quote;
mod minting;
mod proof;
mod state;

pub use melt::{
    AmountlessBolt11, MeltBolt11Request, MeltBolt11Response, MeltQuoteBolt11Options,
    MeltQuoteBolt11Request, MeltQuoteBolt11Response, MeltQuoteState,
};
pub use mint_quote::{MintQuoteRequest, MintQuoteResponse, MintQuoteState};
pub use minting::{
    BlindSignature, BlindedMessage, DleqProofWire, MintTokensRequest, MintTokensResponse,
};
pub use proof::{Proof, SwapRequest, SwapResponse};
pub use state::{ProofSpendState, ProofState, StateCheckRequest, StateCheckResponse};

use serde::Deserialize;
use std::collections::HashMap;

/// Placeholder printed in place of a redacted `Debug` field. Shared by every
/// submodule's hand-written `Debug` impl.
pub(super) const REDACTED: &str = "<redacted>";

// ─── Keysets (NUT-01/NUT-02) ────────────────────────────────────────────────

/// Response from `GET /v1/keys` — all active keysets for a mint.
#[derive(Debug, Clone, Deserialize)]
pub struct KeysResponse {
    pub keysets: Vec<KeySet>,
}

/// A single keyset with per-denomination public keys.
#[derive(Debug, Clone, Deserialize)]
pub struct KeySet {
    /// Keyset identifier (hex).
    pub id: String,
    /// Currency unit (e.g. "sat").
    pub unit: String,
    /// Map from denomination (as string integer) to compressed public key (hex).
    #[serde(default)]
    pub keys: HashMap<String, String>,
    /// Fee in parts per thousand per input (NUT-02). Missing = 0.
    #[serde(default)]
    pub input_fee_ppk: u64,
}

// ─── Mint info (NUT-06) ────────────────────────────────────────────────────

/// Response from `GET /v1/info`.
#[derive(Debug, Clone, Deserialize)]
pub struct MintInfoResponse {
    pub name: Option<String>,
    pub pubkey: String,
    pub version: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub contact: Vec<serde_json::Value>,
}
