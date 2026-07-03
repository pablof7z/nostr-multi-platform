//! `/v1/mint/bolt11` (NUT-04) — request preparation and response
//! finalization.

use std::fmt;

use nostr::secp256k1::{All, Secp256k1};

use super::blinded::{build_blinded_outputs, finalize_blinded_outputs, BlindedOutputSet};
use super::{
    parse_json_response, DleqPolicy, MintHttpMethod, MintHttpOperation, MintHttpRequest,
    MintRawResponse, REDACTED,
};
use crate::cashu::http::blinded::split_amount;
use crate::cashu::types::{KeySet, MintTokensRequest, MintTokensResponse, Proof};
use crate::error::Nip60Error;

/// Everything needed to construct the `POST /v1/mint/bolt11` request and
/// later unblind its response.
pub struct PreparedMintBolt11Request {
    pub http: MintHttpRequest,
    quote: String,
    keyset: KeySet,
    outputs: BlindedOutputSet,
}

impl fmt::Debug for PreparedMintBolt11Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedMintBolt11Request")
            .field("quote", &REDACTED)
            .field("keyset_id", &self.keyset.id)
            .field("outputs", &self.outputs)
            .finish()
    }
}

impl PreparedMintBolt11Request {
    /// The mint quote id this request mints against — needed by a caller
    /// (e.g. the `nmp-wallet` journal) to correlate the eventual result back
    /// to the pending operation it started.
    #[must_use]
    pub fn quote_id(&self) -> &str {
        &self.quote
    }
}

pub fn prepare_mint_bolt11_request(
    quote_id: &str,
    total_amount: u64,
    keyset: &KeySet,
    secp: &Secp256k1<All>,
) -> Result<PreparedMintBolt11Request, Nip60Error> {
    if quote_id.trim().is_empty() {
        return Err(Nip60Error::Invalid(
            "mint quote id must not be empty".into(),
        ));
    }
    if total_amount == 0 {
        return Err(Nip60Error::Invalid(
            "mint request amount must be greater than zero".into(),
        ));
    }
    let denominations = split_amount(total_amount);
    let outputs = build_blinded_outputs(&denominations, &keyset.id, None, secp)?;
    let body = serde_json::to_vec(&MintTokensRequest {
        quote: quote_id.to_string(),
        outputs: outputs.wire.clone(),
    })?;
    Ok(PreparedMintBolt11Request {
        http: MintHttpRequest {
            operation: MintHttpOperation::MintBolt11,
            method: MintHttpMethod::Post,
            path: "/v1/mint/bolt11".to_string(),
            body,
        },
        quote: quote_id.to_string(),
        keyset: keyset.clone(),
        outputs,
    })
}

pub fn finalize_mint_bolt11_response(
    prepared: &PreparedMintBolt11Request,
    raw: &MintRawResponse,
    dleq_policy: DleqPolicy,
    secp: &Secp256k1<All>,
) -> Result<Vec<Proof>, Nip60Error> {
    let resp: MintTokensResponse = parse_json_response(raw, "mint response")?;
    finalize_blinded_outputs(
        &prepared.outputs,
        &prepared.keyset,
        &resp.signatures,
        dleq_policy,
        secp,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cashu::http::mint_http_support::{fixture_keyset, ok, secp};
    use crate::cashu::types::BlindSignature;
    use nostr::secp256k1::{PublicKey, SecretKey};

    /// Round-trips a real mint-signing simulation through prepare/finalize
    /// so the "rejects" tests below have a known-good baseline to tamper.
    pub(crate) fn mint_signatures_for(
        prepared: &PreparedMintBolt11Request,
        mint_sk: &SecretKey,
    ) -> Vec<BlindSignature> {
        let secp = secp();
        prepared
            .outputs
            .wire
            .iter()
            .map(|out| {
                let b_prime = PublicKey::from_slice(&hex::decode(&out.b_prime).unwrap()).unwrap();
                let k_scalar = nostr::secp256k1::Scalar::from(*mint_sk);
                let c_prime = b_prime.mul_tweak(&secp, &k_scalar).unwrap();
                BlindSignature {
                    amount: out.amount,
                    id: out.id.clone(),
                    c_prime: hex::encode(c_prime.serialize()),
                    dleq: None,
                }
            })
            .collect()
    }

    #[test]
    fn prepare_mint_rejects_zero_amount() {
        let (keyset, _) = fixture_keyset();
        let secp = secp();
        let err = prepare_mint_bolt11_request("q1", 0, &keyset, &secp).unwrap_err();
        assert!(matches!(err, Nip60Error::Invalid(_)));
    }

    #[test]
    fn finalize_mint_rejects_signature_count_mismatch() {
        let (keyset, mint_sk) = fixture_keyset();
        let secp = secp();
        let prepared = prepare_mint_bolt11_request("q1", 3, &keyset, &secp).unwrap();
        let mut sigs = mint_signatures_for(&prepared, &mint_sk);
        sigs.pop();
        let raw = ok(serde_json::to_vec(&MintTokensResponse { signatures: sigs })
            .unwrap()
            .as_slice());
        let err =
            finalize_mint_bolt11_response(&prepared, &raw, DleqPolicy::VerifyIfPresent, &secp)
                .unwrap_err();
        assert!(matches!(err, Nip60Error::MintProtocol(_)));
    }

    #[test]
    fn finalize_mint_rejects_wrong_signature_amount() {
        let (keyset, mint_sk) = fixture_keyset();
        let secp = secp();
        let prepared = prepare_mint_bolt11_request("q1", 3, &keyset, &secp).unwrap();
        let mut sigs = mint_signatures_for(&prepared, &mint_sk);
        sigs[0].amount += 1;
        let raw = ok(serde_json::to_vec(&MintTokensResponse { signatures: sigs })
            .unwrap()
            .as_slice());
        let err =
            finalize_mint_bolt11_response(&prepared, &raw, DleqPolicy::VerifyIfPresent, &secp)
                .unwrap_err();
        assert!(matches!(err, Nip60Error::MintProtocol(_)));
    }

    #[test]
    fn finalize_mint_rejects_wrong_keyset_id() {
        let (keyset, mint_sk) = fixture_keyset();
        let secp = secp();
        let prepared = prepare_mint_bolt11_request("q1", 3, &keyset, &secp).unwrap();
        let mut sigs = mint_signatures_for(&prepared, &mint_sk);
        sigs[0].id = "00wrongkeyset".to_string();
        let raw = ok(serde_json::to_vec(&MintTokensResponse { signatures: sigs })
            .unwrap()
            .as_slice());
        let err =
            finalize_mint_bolt11_response(&prepared, &raw, DleqPolicy::VerifyIfPresent, &secp)
                .unwrap_err();
        assert!(matches!(err, Nip60Error::MintProtocol(_)));
    }

    #[test]
    fn finalize_mint_rejects_missing_dleq_when_required() {
        let (keyset, mint_sk) = fixture_keyset();
        let secp = secp();
        let prepared = prepare_mint_bolt11_request("q1", 3, &keyset, &secp).unwrap();
        let sigs = mint_signatures_for(&prepared, &mint_sk); // no dleq attached
        let raw = ok(serde_json::to_vec(&MintTokensResponse { signatures: sigs })
            .unwrap()
            .as_slice());
        let err =
            finalize_mint_bolt11_response(&prepared, &raw, DleqPolicy::Require, &secp).unwrap_err();
        assert!(matches!(err, Nip60Error::Crypto(_)));
    }

    #[test]
    fn finalize_mint_accepts_missing_dleq_when_optional() {
        let (keyset, mint_sk) = fixture_keyset();
        let secp = secp();
        let prepared = prepare_mint_bolt11_request("q1", 3, &keyset, &secp).unwrap();
        let sigs = mint_signatures_for(&prepared, &mint_sk);
        let raw = ok(serde_json::to_vec(&MintTokensResponse { signatures: sigs })
            .unwrap()
            .as_slice());
        let proofs =
            finalize_mint_bolt11_response(&prepared, &raw, DleqPolicy::VerifyIfPresent, &secp)
                .unwrap();
        assert_eq!(proofs.iter().map(|p| p.amount).sum::<u64>(), 3);
    }
}
