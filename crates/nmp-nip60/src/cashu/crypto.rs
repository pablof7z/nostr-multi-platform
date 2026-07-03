//! Cashu Diffie-Hellman Key Exchange (DHKE) and DLEQ proof verification.
//!
//! Implements NUT-00 (blind signatures) and NUT-12 (DLEQ proofs) using
//! secp256k1 via the `nostr` crate's re-exported `secp256k1` dependency.
//!
//! # Cashu DHKE protocol
//!
//! 1. Client picks random secret `x`, computes `Y = hash_to_curve(x)`.
//! 2. Client generates random blinding factor `r`, computes `B' = Y + r*G`.
//! 3. Mint signs: `C' = k * B'`.
//! 4. Client unblinds: `C = C' - r * K` where `K = k*G` is the mint's pubkey.
//! 5. Proof = (keyset_id, amount, secret_x, C).
//!
//! # DLEQ proof (NUT-12)
//!
//! Proves `C' = k*B'` without revealing `k`. Schnorr proof:
//! - Prover picks nonce `r`, computes `R1 = r*G`, `R2 = r*B'`.
//! - Challenge `e = Hash(R1 || R2 || K || C')`.
//! - Response `s = r + e*k`.
//! - Verifier checks: `R1 == s*G - e*K`, `R2 == s*B' - e*C'`.

use nostr::secp256k1::{self, PublicKey, Scalar, Secp256k1, SecretKey};
use sha2::{Digest, Sha256};

use crate::error::Nip60Error;

/// Cashu's domain-separated hash-to-curve (NUT-00).
///
/// Hashes `message` to a secp256k1 point using try-and-increment with
/// the Cashu domain separator `"Secp256k1_HashToCurve_Cashu_"`.
pub fn hash_to_curve(message: &[u8]) -> Result<PublicKey, Nip60Error> {
    // Domain-separated message hash.
    let mut h = Sha256::new();
    h.update(b"Secp256k1_HashToCurve_Cashu_");
    h.update(message);
    let msg_hash = h.finalize();

    // Try-and-increment: find a valid compressed point.
    for counter in 0u32..1000 {
        let mut h2 = Sha256::new();
        h2.update(msg_hash);
        h2.update(counter.to_le_bytes()); // 4-byte little-endian per NUT-00 spec
        let hash = h2.finalize();

        // Build a 33-byte compressed point candidate with prefix 0x02.
        let mut point_bytes = [0u8; 33];
        point_bytes[0] = 0x02;
        point_bytes[1..].copy_from_slice(&hash);

        if let Ok(pk) = PublicKey::from_slice(&point_bytes) {
            return Ok(pk);
        }
    }
    Err(Nip60Error::Crypto(
        "hash_to_curve: no valid point found in 1000 attempts".into(),
    ))
}

/// Produce a blinded message `B' = Y + r*G` from secret `x` and blinding factor `r`.
pub fn blind_message(
    secret_x: &[u8],
    blinding_factor: &SecretKey,
    secp: &Secp256k1<secp256k1::All>,
) -> Result<PublicKey, Nip60Error> {
    let y = hash_to_curve(secret_x)?;
    // B' = Y + r*G → combine(Y, r*G)
    let r_g = PublicKey::from_secret_key(secp, blinding_factor);
    PublicKey::combine_keys(&[&y, &r_g])
        .map_err(|e| Nip60Error::Crypto(format!("blind_message combine: {e}")))
}

/// Unblind a blind signature: `C = C' - r * K`.
///
/// Uses point negation: `C = C' + (-r*K)`.
pub fn unblind_signature(
    c_prime: &PublicKey,
    blinding_factor: &SecretKey,
    mint_pubkey: &PublicKey,
    secp: &Secp256k1<secp256k1::All>,
) -> Result<PublicKey, Nip60Error> {
    // Compute r*K
    let r_scalar = Scalar::from(*blinding_factor);
    let r_k = mint_pubkey
        .mul_tweak(secp, &r_scalar)
        .map_err(|e| Nip60Error::Crypto(format!("r*K mul_tweak: {e}")))?;
    // Negate r*K → -r*K
    let neg_r_k = r_k.negate(secp);
    // C = C' + (-r*K)
    PublicKey::combine_keys(&[c_prime, &neg_r_k])
        .map_err(|e| Nip60Error::Crypto(format!("unblind combine: {e}")))
}

/// DLEQ proof delivered by the mint alongside a blind signature (NUT-12).
#[derive(Debug, Clone)]
pub struct DleqProof {
    /// Challenge scalar `e`.
    pub e: [u8; 32],
    /// Response scalar `s`.
    pub s: [u8; 32],
}

/// Verify a DLEQ proof: the mint proves `C' = k*B'` without revealing `k`.
///
/// Verifier checks:
/// - `R1 = s*G - e*K`
/// - `R2 = s*B' - e*C'`
/// - `e == H(R1 || R2 || K || C')`
pub fn verify_dleq(
    proof: &DleqProof,
    b_prime: &PublicKey,
    c_prime: &PublicKey,
    mint_pubkey: &PublicKey,
    secp: &Secp256k1<secp256k1::All>,
) -> Result<(), Nip60Error> {
    let e_sk = SecretKey::from_slice(&proof.e)
        .map_err(|e| Nip60Error::Crypto(format!("DLEQ e as SK: {e}")))?;
    let e_scalar = Scalar::from(e_sk);
    let s_sk = SecretKey::from_slice(&proof.s)
        .map_err(|e| Nip60Error::Crypto(format!("DLEQ s scalar: {e}")))?;
    let s_scalar = Scalar::from(s_sk);

    // R1 = s*G - e*K  →  s*G + (-(e*K))
    let s_g = PublicKey::from_secret_key(secp, &s_sk);
    let e_k = mint_pubkey
        .mul_tweak(secp, &e_scalar)
        .map_err(|e| Nip60Error::Crypto(format!("DLEQ e*K: {e}")))?;
    let neg_e_k = e_k.negate(secp);
    let r1 = PublicKey::combine_keys(&[&s_g, &neg_e_k])
        .map_err(|e| Nip60Error::Crypto(format!("DLEQ R1 combine: {e}")))?;

    // R2 = s*B' - e*C'  →  s*B' + (-(e*C'))
    let s_b_prime = b_prime
        .mul_tweak(secp, &s_scalar)
        .map_err(|e| Nip60Error::Crypto(format!("DLEQ s*B': {e}")))?;
    let e_c_prime = c_prime
        .mul_tweak(secp, &e_scalar)
        .map_err(|e| Nip60Error::Crypto(format!("DLEQ e*C': {e}")))?;
    let neg_e_c_prime = e_c_prime.negate(secp);
    let r2 = PublicKey::combine_keys(&[&s_b_prime, &neg_e_c_prime])
        .map_err(|e| Nip60Error::Crypto(format!("DLEQ R2 combine: {e}")))?;

    // Cashu NUT-12 (nutshell reference): SHA256 of UTF-8 hex of UNCOMPRESSED points
    // hash_e(R1, R2, K, C') → SHA256(hex(R1_uncompressed) + hex(R2_uncompressed) + hex(K_uncompressed) + hex(C'_uncompressed))
    // Note: nutshell uses uncompressed (65-byte, 04||x||y) NOT compressed (33-byte), and
    // the hash input is the concatenated hex string encoded as UTF-8 bytes.
    let e_str = format!(
        "{}{}{}{}",
        hex::encode(r1.serialize_uncompressed()),      // 130 hex chars
        hex::encode(r2.serialize_uncompressed()),
        hex::encode(mint_pubkey.serialize_uncompressed()),   // K (mint pubkey), not B'
        hex::encode(c_prime.serialize_uncompressed()),
    );
    let mut h = Sha256::new();
    h.update(e_str.as_bytes());
    let e_expected: [u8; 32] = h.finalize().into();

    if e_expected != proof.e {
        return Err(Nip60Error::Crypto(format!(
            "DLEQ proof invalid: e mismatch (got {}, expected {})",
            hex::encode(proof.e),
            hex::encode(e_expected),
        )));
    }
    Ok(())
}

/// A random 32-byte secret (used as the proof secret `x`).
pub fn random_secret() -> [u8; 32] {
    let sk = SecretKey::new(&mut nostr::secp256k1::rand::thread_rng());
    sk.secret_bytes()
}

/// [`random_secret`], hex-encoded — the ordinary (non-P2PK) output-proof
/// secret shape a swap's change outputs use. A thin convenience so callers
/// outside this crate (e.g. `nmp-wallet`'s nutzap send/redeem flows) don't
/// need their own `hex` dependency just for this one encode.
#[must_use]
pub fn random_secret_hex() -> String {
    hex::encode(random_secret())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::secp256k1::Secp256k1;

    #[test]
    fn hash_to_curve_returns_valid_point() {
        let pt = hash_to_curve(b"test message").expect("hash_to_curve must succeed");
        // Round-trip through compressed serialisation.
        let bytes = pt.serialize();
        let pt2 = PublicKey::from_slice(&bytes).expect("deser");
        assert_eq!(pt, pt2);
    }

    #[test]
    fn blind_unblind_roundtrip() {
        let secp = Secp256k1::new();
        let secret = b"cashu test secret";
        let r = SecretKey::new(&mut nostr::secp256k1::rand::thread_rng());

        // Simulate mint private key `k` and public key `K`.
        let k = SecretKey::new(&mut nostr::secp256k1::rand::thread_rng());
        let mint_pk = PublicKey::from_secret_key(&secp, &k);

        // Client: blind
        let b_prime = blind_message(secret, &r, &secp).expect("blind");

        // Mint: sign  C' = k * B'
        let k_scalar = Scalar::from(k);
        let c_prime = b_prime.mul_tweak(&secp, &k_scalar).expect("mint sign");

        // Client: unblind → C
        let c = unblind_signature(&c_prime, &r, &mint_pk, &secp).expect("unblind");

        // Verify: C should equal k * Y where Y = hash_to_curve(secret)
        let y = hash_to_curve(secret).expect("htc");
        let k_scalar2 = Scalar::from(k);
        let expected_c = y.mul_tweak(&secp, &k_scalar2).expect("k*Y");
        assert_eq!(c, expected_c, "unblinded C must equal k*Y");
    }
}
