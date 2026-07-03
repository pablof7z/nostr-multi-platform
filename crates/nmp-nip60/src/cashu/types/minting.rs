//! NUT-04 mint-tokens request/response types + the NUT-12 DLEQ wire format
//! shared with swap ([`super::proof`]).

use std::fmt;

use serde::{Deserialize, Serialize};

use super::REDACTED;

/// A blinded message sent to the mint for signing.
#[derive(Debug, Clone, Serialize)]
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
#[derive(Serialize)]
pub struct MintTokensRequest {
    pub quote: String,
    pub outputs: Vec<BlindedMessage>,
}

impl fmt::Debug for MintTokensRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MintTokensRequest")
            .field("quote", &REDACTED)
            .field("output_count", &self.outputs.len())
            .finish()
    }
}

/// A blind signature returned by the mint.
#[derive(Debug, Clone, Deserialize, Serialize)]
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
#[derive(Clone, Deserialize, Serialize)]
pub struct DleqProofWire {
    pub e: String,
    pub s: String,
    /// Blinding factor (client's `r`) — required when spending proofs as inputs.
    /// Secret: reveals the randomness used to blind this proof's mint
    /// signature request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r: Option<String>,
}

impl fmt::Debug for DleqProofWire {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DleqProofWire")
            .field("e", &self.e)
            .field("s", &self.s)
            .field("r", &self.r.as_ref().map(|_| REDACTED))
            .finish()
    }
}

/// Response from `POST /v1/mint/bolt11`.
#[derive(Debug, Deserialize, Serialize)]
pub struct MintTokensResponse {
    pub signatures: Vec<BlindSignature>,
}
