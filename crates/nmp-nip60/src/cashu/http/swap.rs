//! `/v1/swap` (NUT-03) — request preparation and response finalization.

use std::fmt;

use nostr::secp256k1::{All, Secp256k1};

use super::blinded::{build_blinded_outputs, finalize_blinded_outputs, BlindedOutputSet};
use super::{
    parse_json_response, DleqPolicy, MintHttpMethod, MintHttpOperation, MintHttpRequest,
    MintRawResponse,
};
use crate::cashu::types::{KeySet, Proof, SwapRequest, SwapResponse};
use crate::error::Nip60Error;

pub struct PreparedSwapRequest {
    pub http: MintHttpRequest,
    keyset: KeySet,
    outputs: BlindedOutputSet,
}

impl fmt::Debug for PreparedSwapRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedSwapRequest")
            .field("keyset_id", &self.keyset.id)
            .field("outputs", &self.outputs)
            .finish()
    }
}

pub fn prepare_swap_request(
    inputs: Vec<Proof>,
    output_amounts: Vec<u64>,
    output_secrets: Option<Vec<String>>,
    keyset: &KeySet,
    secp: &Secp256k1<All>,
) -> Result<PreparedSwapRequest, Nip60Error> {
    if inputs.is_empty() {
        return Err(Nip60Error::Invalid(
            "swap requires at least one input proof".into(),
        ));
    }
    if output_amounts.is_empty() {
        return Err(Nip60Error::Invalid(
            "swap requires at least one output amount".into(),
        ));
    }
    if output_amounts.contains(&0) {
        return Err(Nip60Error::Invalid(
            "swap output amounts must all be greater than zero".into(),
        ));
    }
    if let Some(ref secrets) = output_secrets {
        if secrets.len() != output_amounts.len() {
            return Err(Nip60Error::Invalid(format!(
                "swap output_secrets length {} does not match output_amounts length {}",
                secrets.len(),
                output_amounts.len()
            )));
        }
    }
    // Defensive overflow guard — a caller-supplied amount list is untrusted
    // input (proof selection upstream could in principle hand us an
    // adversarial list); reject rather than wrap.
    output_amounts
        .iter()
        .try_fold(0u64, |acc, &a| acc.checked_add(a))
        .ok_or_else(|| Nip60Error::Invalid("swap output amounts overflow u64".into()))?;

    let outputs =
        build_blinded_outputs(&output_amounts, &keyset.id, output_secrets.as_deref(), secp)?;
    let body = serde_json::to_vec(&SwapRequest {
        inputs,
        outputs: outputs.wire.clone(),
    })?;
    Ok(PreparedSwapRequest {
        http: MintHttpRequest {
            operation: MintHttpOperation::Swap,
            method: MintHttpMethod::Post,
            path: "/v1/swap".to_string(),
            body,
        },
        keyset: keyset.clone(),
        outputs,
    })
}

pub fn finalize_swap_response(
    prepared: &PreparedSwapRequest,
    raw: &MintRawResponse,
    dleq_policy: DleqPolicy,
    secp: &Secp256k1<All>,
) -> Result<Vec<Proof>, Nip60Error> {
    let resp: SwapResponse = parse_json_response(raw, "swap response")?;
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
    use crate::cashu::crypto::random_secret;
    use crate::cashu::http::mint_http_support::{fixture_keyset, ok, secp};
    use crate::cashu::types::{BlindSignature, DleqProofWire};
    use nostr::secp256k1::{PublicKey, SecretKey};

    fn mint_signatures_for_swap(
        prepared: &PreparedSwapRequest,
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

    fn fixture_input(keyset_id: &str) -> Proof {
        Proof {
            amount: 4,
            id: keyset_id.to_string(),
            secret: hex::encode(random_secret()),
            c: "02".to_string() + &"aa".repeat(32),
            dleq: None,
            witness: None,
        }
    }

    #[test]
    fn prepare_swap_rejects_overflow_output_amounts() {
        let (keyset, _) = fixture_keyset();
        let secp = secp();
        let err = prepare_swap_request(
            vec![fixture_input(&keyset.id)],
            vec![u64::MAX, u64::MAX],
            None,
            &keyset,
            &secp,
        )
        .unwrap_err();
        assert!(matches!(err, Nip60Error::Invalid(_)));
    }

    #[test]
    fn prepare_swap_rejects_empty_inputs() {
        let (keyset, _) = fixture_keyset();
        let secp = secp();
        assert!(prepare_swap_request(vec![], vec![4], None, &keyset, &secp).is_err());
    }

    #[test]
    fn prepare_swap_rejects_zero_amount_output() {
        let (keyset, _) = fixture_keyset();
        let secp = secp();
        let err = prepare_swap_request(
            vec![fixture_input(&keyset.id)],
            vec![4, 0],
            None,
            &keyset,
            &secp,
        )
        .unwrap_err();
        assert!(matches!(err, Nip60Error::Invalid(_)));
    }

    #[test]
    fn finalize_swap_rejects_tampered_dleq_proof() {
        let (keyset, mint_sk) = fixture_keyset();
        let secp = secp();
        let prepared = prepare_swap_request(
            vec![fixture_input(&keyset.id)],
            vec![4],
            None,
            &keyset,
            &secp,
        )
        .unwrap();
        let mut sigs = mint_signatures_for_swap(&prepared, &mint_sk);
        // Attach a well-formed but bogus DLEQ proof (correct hex/length,
        // wrong Schnorr challenge) to an otherwise-genuine mint signature —
        // a mint (or a MITM) that tampers with the DLEQ transcript must be
        // rejected even though the C' itself unblinds "successfully"
        // (unblinding is pure arithmetic; DLEQ is the only thing that
        // proves the mint actually produced this exact signature).
        sigs[0].dleq = Some(DleqProofWire {
            e: "00".repeat(32),
            s: "00".repeat(32),
            r: None,
        });
        let raw = ok(serde_json::to_vec(&SwapResponse { signatures: sigs })
            .unwrap()
            .as_slice());
        let err = finalize_swap_response(&prepared, &raw, DleqPolicy::Require, &secp).unwrap_err();
        assert!(matches!(err, Nip60Error::Crypto(_)));
    }
}
