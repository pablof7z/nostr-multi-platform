//! Cashu HTTP API request/response types (NUT-01 through NUT-12).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Keysets ───────────────────────────────────────────────────────────────

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

// ─── Mint quote (NUT-04) ───────────────────────────────────────────────────

/// Request body for `POST /v1/mint/quote/bolt11`.
#[derive(Debug, Serialize)]
pub struct MintQuoteRequest {
    pub amount: u64,
    pub unit: String,
}

/// Response from `POST /v1/mint/quote/bolt11`.
#[derive(Debug, Clone, Deserialize)]
pub struct MintQuoteResponse {
    pub quote: String,
    pub request: String, // bolt11 invoice
    pub state: String,   // "UNPAID" | "PAID" | "ISSUED"
    /// Expiry timestamp. Some mints return `null` here.
    #[serde(default)]
    pub expiry: Option<u64>,
    #[serde(default)]
    pub paid: bool,
}

// ─── Minting (NUT-04) ──────────────────────────────────────────────────────

/// A blinded message sent to the mint for signing.
#[derive(Debug, Serialize)]
pub struct BlindedMessage {
    /// Amount this message represents.
    pub amount: u64,
    /// Keyset ID.
    pub id: String,
    /// The blinded point `B'` as a compressed hex pubkey.
    #[serde(rename = "B_")]
    pub b_prime: String,
}

/// Request body for `POST /v1/mint/bolt11`.
#[derive(Debug, Serialize)]
pub struct MintTokensRequest {
    pub quote: String,
    pub outputs: Vec<BlindedMessage>,
}

/// A blind signature returned by the mint.
#[derive(Debug, Clone, Deserialize)]
pub struct BlindSignature {
    pub amount: u64,
    pub id: String,
    /// The blind signature point `C'` (compressed hex pubkey).
    #[serde(rename = "C_")]
    pub c_prime: String,
    /// Optional DLEQ proof (NUT-12).
    #[serde(default)]
    pub dleq: Option<DleqProofWire>,
}

/// Wire format for a DLEQ proof.
///
/// When sending proofs as inputs to a swap, `r` (the blinding factor used at mint time)
/// must be included so the mint can re-verify the DLEQ proof. The mint uses it to
/// recompute `B' = Y + r*G` and verify `C' = k*B'`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DleqProofWire {
    pub e: String,
    pub s: String,
    /// Blinding factor (client's `r`) — required when spending proofs as inputs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r: Option<String>,
}

/// Response from `POST /v1/mint/bolt11`.
#[derive(Debug, Deserialize)]
pub struct MintTokensResponse {
    pub signatures: Vec<BlindSignature>,
}

// ─── Swap (NUT-03) ─────────────────────────────────────────────────────────

/// A proof to be spent (input to a swap).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proof {
    pub amount: u64,
    pub id: String,
    /// The proof secret (random bytes hex, or P2PK spending condition JSON).
    pub secret: String,
    /// The unblinded signature `C` (compressed hex pubkey).
    #[serde(rename = "C")]
    pub c: String,
    /// Optional DLEQ proof (NUT-12), present if the proof was minted with DLEQ.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dleq: Option<DleqProofWire>,
    /// Optional witness for P2PK spending conditions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub witness: Option<serde_json::Value>,
}

/// Request body for `POST /v1/swap`.
#[derive(Debug, Serialize)]
pub struct SwapRequest {
    pub inputs: Vec<Proof>,
    pub outputs: Vec<BlindedMessage>,
}

/// Response from `POST /v1/swap`.
#[derive(Debug, Deserialize)]
pub struct SwapResponse {
    pub signatures: Vec<BlindSignature>,
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

// ─── Proof state check (NUT-07) ────────────────────────────────────────────

/// State check request.
#[derive(Debug, Serialize)]
pub struct StateCheckRequest {
    #[serde(rename = "Ys")]
    pub ys: Vec<String>, // Y = hash_to_curve(secret), compressed hex
}

#[derive(Debug, Deserialize)]
pub struct ProofState {
    #[serde(rename = "Y")]
    pub y: String,
    pub state: String, // "UNSPENT" | "SPENT" | "PENDING"
    #[serde(default)]
    pub witness: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StateCheckResponse {
    pub states: Vec<ProofState>,
}
