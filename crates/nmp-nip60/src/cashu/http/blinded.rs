//! The shared blind/unblind + DLEQ-verify engine both minting
//! ([`super::mint`]) and swapping ([`super::swap`]) build on (NUT-00/NUT-12).

use std::fmt;

use nostr::secp256k1::{All, PublicKey, Secp256k1, SecretKey};

use super::REDACTED;
use crate::cashu::crypto::{blind_message, unblind_signature, verify_dleq, DleqProof};
use crate::cashu::http::keyset::build_pubkey_map;
use crate::cashu::http::DleqPolicy;
use crate::cashu::types::{BlindSignature, BlindedMessage, DleqProofWire, KeySet, Proof};
use crate::error::Nip60Error;

/// Split `amount` into powers-of-2 denominations (smallest first). The sum
/// of the returned denominations always exactly equals `amount` — each bit
/// of `amount` becomes at most one denomination, so this can never overflow
/// `u64` (it can only ever reconstruct a value `amount` already held).
#[must_use]
pub fn split_amount(amount: u64) -> Vec<u64> {
    let mut denominations = Vec::new();
    for bit in 0..64u64 {
        let denom = 1 << bit;
        if amount & denom != 0 {
            denominations.push(denom);
        }
    }
    denominations
}

/// Per-output blinding state generated while preparing a mint or swap
/// request — needed to unblind the mint's signatures once the response
/// arrives. Not `Clone`/`Copy`: this is one-shot, single-use secret state.
pub(super) struct BlindedOutputSet {
    pub(super) denominations: Vec<u64>,
    /// The Cashu secret (`x`) for each output, in the same order as
    /// `denominations`.
    pub(super) secrets: Vec<String>,
    pub(super) blinding_factors: Vec<SecretKey>,
    pub(super) b_primes_hex: Vec<String>,
    pub(super) wire: Vec<BlindedMessage>,
}

impl fmt::Debug for BlindedOutputSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BlindedOutputSet")
            .field("denominations", &self.denominations)
            .field("secrets", &REDACTED)
            .field("blinding_factors", &REDACTED)
            .finish()
    }
}

/// Generate one blinded message per entry in `denominations`, reusing
/// `explicit_secrets[i]` when provided (P2PK spending conditions supply
/// their own secret string) or a fresh random secret otherwise.
pub(super) fn build_blinded_outputs(
    denominations: &[u64],
    keyset_id: &str,
    explicit_secrets: Option<&[String]>,
    secp: &Secp256k1<All>,
) -> Result<BlindedOutputSet, Nip60Error> {
    let n = denominations.len();
    let mut secrets = Vec::with_capacity(n);
    let mut blinding_factors = Vec::with_capacity(n);
    let mut b_primes_hex = Vec::with_capacity(n);
    let mut wire = Vec::with_capacity(n);

    for (i, &amount) in denominations.iter().enumerate() {
        let secret = match explicit_secrets {
            Some(secrets) => secrets[i].clone(),
            None => hex::encode(crate::cashu::crypto::random_secret()),
        };
        let r = SecretKey::new(&mut nostr::secp256k1::rand::thread_rng());
        let b_prime = blind_message(secret.as_bytes(), &r, secp)?;
        let b_prime_hex = hex::encode(b_prime.serialize());
        wire.push(BlindedMessage {
            amount,
            id: keyset_id.to_string(),
            b_prime: b_prime_hex.clone(),
        });
        secrets.push(secret);
        blinding_factors.push(r);
        b_primes_hex.push(b_prime_hex);
    }

    Ok(BlindedOutputSet {
        denominations: denominations.to_vec(),
        secrets,
        blinding_factors,
        b_primes_hex,
        wire,
    })
}

/// Unblind + (per `dleq_policy`) verify every signature in `signatures`
/// against the matching entry in `outputs`, producing spendable [`Proof`]s.
/// Fails closed on any mismatch (wrong count, wrong amount, wrong keyset id,
/// missing/invalid/tampered DLEQ) rather than returning a partial result.
pub(super) fn finalize_blinded_outputs(
    outputs: &BlindedOutputSet,
    keyset: &KeySet,
    signatures: &[BlindSignature],
    dleq_policy: DleqPolicy,
    secp: &Secp256k1<All>,
) -> Result<Vec<Proof>, Nip60Error> {
    if signatures.len() != outputs.denominations.len() {
        return Err(Nip60Error::MintProtocol(format!(
            "mint returned {} signatures, expected {}",
            signatures.len(),
            outputs.denominations.len()
        )));
    }
    let mint_pubkey_for_amount = build_pubkey_map(keyset)?;
    signatures
        .iter()
        .enumerate()
        .map(|(i, sig)| {
            unblind_one(
                i,
                outputs,
                keyset,
                sig,
                Some(outputs.denominations[i]),
                &mint_pubkey_for_amount,
                dleq_policy,
                secp,
            )
        })
        .collect()
}

/// Build `count` blank (denomination-unknown) blinded outputs for NUT-08
/// fee-reserve change. Each wire `amount` is a `0` placeholder only —
/// [`finalize_blank_outputs`] never compares a returned signature's amount
/// against it; the mint alone decides each change output's real
/// denomination (it is making the change, so it has full latitude — the
/// client's only guarantee is the DLEQ proof over whatever amount the mint
/// asserts, exactly like NUT-08 intends).
pub(super) fn build_blank_outputs(
    count: usize,
    keyset_id: &str,
    secp: &Secp256k1<All>,
) -> Result<BlindedOutputSet, Nip60Error> {
    let placeholders = vec![0u64; count];
    build_blinded_outputs(&placeholders, keyset_id, None, secp)
}

/// Number of blank NUT-08 change outputs needed to represent any value in
/// `0..=fee_reserve` using power-of-two denominations: `ceil(log2(fee_reserve))`,
/// floored at `1` once `fee_reserve > 0` (matches the reference NUT-08 wallet
/// formula, e.g. cashu-ts's `getNumberOfBlankOutputs`). `0` when there is no
/// possible change at all.
#[must_use]
pub(super) fn blank_output_count(fee_reserve: u64) -> usize {
    if fee_reserve == 0 {
        return 0;
    }
    // `u64::BITS - leading_zeros` = floor(log2(n)) + 1 for n > 0.
    let bits = u64::BITS - fee_reserve.leading_zeros();
    let ceil_log2 = if fee_reserve.is_power_of_two() {
        bits - 1
    } else {
        bits
    };
    ceil_log2.max(1) as usize
}

/// Unblind + (per `dleq_policy`) verify every signature in `signatures`
/// against blank outputs built by [`build_blank_outputs`] — the NUT-08
/// change leg of a melt. Unlike [`finalize_blinded_outputs`], the mint (not
/// this client) decides each output's denomination, so a signature's
/// `amount` is trusted (subject to DLEQ verification) rather than checked
/// against a value fixed ahead of time. The mint may return fewer
/// signatures than blank outputs offered (it uses only as many as it needs)
/// but never more.
pub(super) fn finalize_blank_outputs(
    outputs: &BlindedOutputSet,
    keyset: &KeySet,
    signatures: &[BlindSignature],
    dleq_policy: DleqPolicy,
    secp: &Secp256k1<All>,
) -> Result<Vec<Proof>, Nip60Error> {
    if signatures.len() > outputs.denominations.len() {
        return Err(Nip60Error::MintProtocol(format!(
            "mint returned {} change signatures, more than the {} blank outputs offered",
            signatures.len(),
            outputs.denominations.len()
        )));
    }
    let mint_pubkey_for_amount = build_pubkey_map(keyset)?;
    signatures
        .iter()
        .enumerate()
        .map(|(i, sig)| {
            unblind_one(
                i,
                outputs,
                keyset,
                sig,
                None,
                &mint_pubkey_for_amount,
                dleq_policy,
                secp,
            )
        })
        .collect()
}

/// Unblind + (per `dleq_policy`) verify one mint signature at index `i`
/// (within `outputs`) into a spendable [`Proof`]. Shared by
/// [`finalize_blinded_outputs`] (`expected_amount: Some(_)` — reject on
/// mismatch) and [`finalize_blank_outputs`] (`expected_amount: None` — the
/// mint decides, `sig.amount` is trusted subject to DLEQ verification).
#[allow(clippy::too_many_arguments)]
fn unblind_one(
    i: usize,
    outputs: &BlindedOutputSet,
    keyset: &KeySet,
    sig: &BlindSignature,
    expected_amount: Option<u64>,
    mint_pubkey_for_amount: &std::collections::HashMap<u64, PublicKey>,
    dleq_policy: DleqPolicy,
    secp: &Secp256k1<All>,
) -> Result<Proof, Nip60Error> {
    if let Some(expected) = expected_amount {
        if sig.amount != expected {
            return Err(Nip60Error::MintProtocol(format!(
                "mint signature #{i} carries amount {}, expected {expected}",
                sig.amount
            )));
        }
    }
    if sig.id != keyset.id {
        return Err(Nip60Error::MintProtocol(format!(
            "mint signature #{i} carries keyset id {}, expected {}",
            sig.id, keyset.id
        )));
    }
    let amount = sig.amount;
    let mint_pk = mint_pubkey_for_amount
        .get(&amount)
        .ok_or_else(|| Nip60Error::Crypto(format!("no mint pubkey for amount {amount}")))?;

    let c_prime_bytes = hex::decode(&sig.c_prime)
        .map_err(|e| Nip60Error::Crypto(format!("C' decode (sig #{i}): {e}")))?;
    let c_prime = PublicKey::from_slice(&c_prime_bytes)
        .map_err(|e| Nip60Error::Crypto(format!("C' parse (sig #{i}): {e}")))?;

    match (&sig.dleq, dleq_policy) {
        (None, DleqPolicy::Require) => {
            return Err(Nip60Error::Crypto(format!(
                "mint signature #{i} is missing a required DLEQ proof"
            )));
        }
        (Some(dleq_wire), _) => {
            let b_prime_bytes = hex::decode(&outputs.b_primes_hex[i])
                .map_err(|e| Nip60Error::Crypto(format!("B' decode (sig #{i}): {e}")))?;
            let b_prime_pt = PublicKey::from_slice(&b_prime_bytes)
                .map_err(|e| Nip60Error::Crypto(format!("B' parse (sig #{i}): {e}")))?;
            let dleq = wire_to_dleq(dleq_wire)?;
            verify_dleq(
                &dleq,
                &b_prime_pt,
                &c_prime,
                mint_pk,
                amount,
                &keyset.id,
                secp,
            )
            .map_err(|e| Nip60Error::Crypto(format!("DLEQ verify failed (sig #{i}): {e}")))?;
        }
        (None, DleqPolicy::VerifyIfPresent) => {}
    }

    let c = unblind_signature(&c_prime, &outputs.blinding_factors[i], mint_pk, secp)?;

    let dleq_with_r = sig.dleq.as_ref().map(|d| DleqProofWire {
        e: d.e.clone(),
        s: d.s.clone(),
        r: Some(hex::encode(outputs.blinding_factors[i].secret_bytes())),
    });

    Ok(Proof {
        amount,
        id: keyset.id.clone(),
        secret: outputs.secrets[i].clone(),
        c: hex::encode(c.serialize()),
        dleq: dleq_with_r,
        witness: None,
    })
}

fn wire_to_dleq(w: &DleqProofWire) -> Result<DleqProof, Nip60Error> {
    let e_bytes = hex::decode(&w.e).map_err(|e| Nip60Error::Crypto(format!("DLEQ e hex: {e}")))?;
    let s_bytes = hex::decode(&w.s).map_err(|e| Nip60Error::Crypto(format!("DLEQ s hex: {e}")))?;
    if e_bytes.len() != 32 || s_bytes.len() != 32 {
        return Err(Nip60Error::Crypto(format!(
            "DLEQ scalar wrong length: e={} s={}",
            e_bytes.len(),
            s_bytes.len()
        )));
    }
    let mut e = [0u8; 32];
    let mut s = [0u8; 32];
    e.copy_from_slice(&e_bytes);
    s.copy_from_slice(&s_bytes);
    Ok(DleqProof { e, s })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cashu::http::mint_http_support::fixture_keyset;

    #[test]
    fn blank_output_count_matches_reference_formula() {
        assert_eq!(blank_output_count(0), 0);
        assert_eq!(blank_output_count(1), 1);
        assert_eq!(blank_output_count(2), 1);
        assert_eq!(blank_output_count(3), 2);
        assert_eq!(blank_output_count(4), 2);
        assert_eq!(blank_output_count(5), 3);
        assert_eq!(blank_output_count(100), 7);
    }

    fn mint_signature_for(
        b_prime_hex: &str,
        id: &str,
        amount: u64,
        mint_sk: &nostr::secp256k1::SecretKey,
        secp: &Secp256k1<All>,
    ) -> BlindSignature {
        let b_prime = PublicKey::from_slice(&hex::decode(b_prime_hex).unwrap()).unwrap();
        let k_scalar = nostr::secp256k1::Scalar::from(*mint_sk);
        let c_prime = b_prime.mul_tweak(secp, &k_scalar).unwrap();
        BlindSignature {
            amount,
            id: id.to_string(),
            c_prime: hex::encode(c_prime.serialize()),
            dleq: None,
        }
    }

    #[test]
    fn finalize_blank_outputs_trusts_mint_chosen_denominations() {
        let (keyset, mint_sk) = fixture_keyset();
        let secp = Secp256k1::new();
        let outputs = build_blank_outputs(3, &keyset.id, &secp).unwrap();
        // The mint only uses 2 of the 3 blank outputs, choosing amounts 4
        // and 1 — values the client never asserted ahead of time.
        let sigs = vec![
            mint_signature_for(&outputs.b_primes_hex[0], &keyset.id, 4, &mint_sk, &secp),
            mint_signature_for(&outputs.b_primes_hex[1], &keyset.id, 1, &mint_sk, &secp),
        ];
        let proofs =
            finalize_blank_outputs(&outputs, &keyset, &sigs, DleqPolicy::VerifyIfPresent, &secp)
                .expect("mint may use fewer blank outputs than offered");
        assert_eq!(proofs.iter().map(|p| p.amount).sum::<u64>(), 5);
    }

    #[test]
    fn finalize_blank_outputs_rejects_more_signatures_than_offered() {
        let (keyset, mint_sk) = fixture_keyset();
        let secp = Secp256k1::new();
        let outputs = build_blank_outputs(1, &keyset.id, &secp).unwrap();
        let sigs = vec![
            mint_signature_for(&outputs.b_primes_hex[0], &keyset.id, 1, &mint_sk, &secp),
            mint_signature_for(&outputs.b_primes_hex[0], &keyset.id, 1, &mint_sk, &secp),
        ];
        let err =
            finalize_blank_outputs(&outputs, &keyset, &sigs, DleqPolicy::VerifyIfPresent, &secp)
                .unwrap_err();
        assert!(matches!(err, Nip60Error::MintProtocol(_)));
    }

    #[test]
    fn finalize_blank_outputs_rejects_unknown_denomination() {
        let (keyset, mint_sk) = fixture_keyset();
        let secp = Secp256k1::new();
        let outputs = build_blank_outputs(1, &keyset.id, &secp).unwrap();
        // 3 is not among the fixture keyset's power-of-two denominations.
        let sigs = vec![mint_signature_for(
            &outputs.b_primes_hex[0],
            &keyset.id,
            3,
            &mint_sk,
            &secp,
        )];
        let err =
            finalize_blank_outputs(&outputs, &keyset, &sigs, DleqPolicy::VerifyIfPresent, &secp)
                .unwrap_err();
        assert!(matches!(err, Nip60Error::Crypto(_)));
    }
}
