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
    let mut proofs = Vec::with_capacity(outputs.denominations.len());

    for (i, sig) in signatures.iter().enumerate() {
        let expected_amount = outputs.denominations[i];
        if sig.amount != expected_amount {
            return Err(Nip60Error::MintProtocol(format!(
                "mint signature #{i} carries amount {}, expected {expected_amount}",
                sig.amount
            )));
        }
        if sig.id != keyset.id {
            return Err(Nip60Error::MintProtocol(format!(
                "mint signature #{i} carries keyset id {}, expected {}",
                sig.id, keyset.id
            )));
        }
        let mint_pk = mint_pubkey_for_amount
            .get(&expected_amount)
            .ok_or_else(|| {
                Nip60Error::Crypto(format!("no mint pubkey for amount {expected_amount}"))
            })?;

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
                verify_dleq(&dleq, &b_prime_pt, &c_prime, mint_pk, secp).map_err(|e| {
                    Nip60Error::Crypto(format!("DLEQ verify failed (sig #{i}): {e}"))
                })?;
            }
            (None, DleqPolicy::VerifyIfPresent) => {}
        }

        let c = unblind_signature(&c_prime, &outputs.blinding_factors[i], mint_pk, secp)?;

        let dleq_with_r = sig.dleq.as_ref().map(|d| DleqProofWire {
            e: d.e.clone(),
            s: d.s.clone(),
            r: Some(hex::encode(outputs.blinding_factors[i].secret_bytes())),
        });

        proofs.push(Proof {
            amount: expected_amount,
            id: keyset.id.clone(),
            secret: outputs.secrets[i].clone(),
            c: hex::encode(c.serialize()),
            dleq: dleq_with_r,
            witness: None,
        });
    }

    Ok(proofs)
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
