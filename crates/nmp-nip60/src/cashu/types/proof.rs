//! The [`Proof`] type (spendable ecash) + NUT-03 swap request/response.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::minting::{BlindSignature, BlindedMessage, DleqProofWire};
use super::REDACTED;

/// A proof to be spent (input to a swap).
#[derive(Clone, Serialize, Deserialize)]
pub struct Proof {
    pub amount: u64,
    pub id: String,
    /// The proof secret (random bytes hex, or P2PK spending condition JSON).
    /// Secret: this is spendable ecash — anyone who learns it can claim the
    /// value, so it must never appear in a log line.
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

impl fmt::Debug for Proof {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Proof")
            .field("amount", &self.amount)
            .field("id", &self.id)
            .field("secret", &REDACTED)
            .field("c", &self.c)
            .field("dleq", &self.dleq)
            .field("has_witness", &self.witness.is_some())
            .finish()
    }
}

/// Request body for `POST /v1/swap`.
#[derive(Debug, Serialize)]
pub struct SwapRequest {
    pub inputs: Vec<Proof>,
    pub outputs: Vec<BlindedMessage>,
}

/// Response from `POST /v1/swap`.
#[derive(Debug, Deserialize, Serialize)]
pub struct SwapResponse {
    pub signatures: Vec<BlindSignature>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_debug_redacts_secret_and_blinding_factor() {
        let proof = Proof {
            amount: 4,
            id: "00abc".into(),
            secret: "top-secret-proof-secret".into(),
            c: "02cc".into(),
            dleq: Some(DleqProofWire {
                e: "ee".into(),
                s: "ss".into(),
                r: Some("top-secret-blinding-factor".into()),
            }),
            witness: None,
        };
        let debug = format!("{proof:?}");
        assert!(!debug.contains("top-secret-proof-secret"));
        assert!(!debug.contains("top-secret-blinding-factor"));
    }
}
