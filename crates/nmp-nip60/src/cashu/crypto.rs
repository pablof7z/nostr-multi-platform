//! Cashu Diffie-Hellman Key Exchange (DHKE) and DLEQ proof verification.
//!
//! Thin adapters over the audited `cashu` crate (part of `cashubtc/cdk` — the
//! pure primitives layer, NOT the async `cdk` wallet crate) for NUT-00
//! (blind signatures) and NUT-12 (DLEQ proofs). This module used to hand-roll
//! this math directly against `nostr::secp256k1`; money-safety reviews found
//! real bugs in that hand-rolled version (#2933 DLEQ fail-open, #2951/#2934
//! proof-strand issues), and the standing rule is "no scratch crypto — use
//! audited crates; nmp-nipXX = thin adapters". This module is now that thin
//! adapter: it converts between this workspace's wire types (hex strings,
//! `nostr::secp256k1::{PublicKey, SecretKey}`) and the `cashu` crate's own
//! typed primitives, and defers all curve math to `cashu`.
//!
//! `cashu`'s own secp256k1 stack (pulled in via `bitcoin` v0.32) resolves to
//! the exact same `secp256k1` v0.29.1 / `secp256k1-sys` v0.10.1 this
//! workspace already uses through the `nostr` crate (see `Cargo.lock` — a
//! single entry for each, i.e. one deduplicated compiled crate), and is
//! confirmed wasm32-clean in this workspace (see the `cashu` dependency
//! comment in `crates/nmp-nip60/Cargo.toml`) — this crate's zero-relay-I/O,
//! wasm32-clean contract is unaffected by this swap.
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
//! Proves `C' = k*B'` without revealing `k`. Verified via
//! `cashu::nuts::nut00::BlindSignature::verify_dleq`, which fails closed
//! (`Err(MissingDleqProof)`) whenever no DLEQ proof is attached — this is the
//! #2933 requirement, and it holds here too since [`verify_dleq`] always
//! attaches the caller-supplied [`DleqProof`] to the `cashu::BlindSignature`
//! it constructs (callers only call this function once they've already
//! decided — per their own `DleqPolicy` — that a DLEQ proof is required and
//! present; see `cashu/http/blinded.rs`).

use nostr::secp256k1::{All, PublicKey, Secp256k1, SecretKey};

use crate::error::Nip60Error;

/// Convert a workspace [`PublicKey`] (compressed 33-byte point) to the
/// `cashu` crate's newtype via a raw byte round-trip. Deliberately not
/// relying on the two crates' `secp256k1` dependencies being the exact same
/// compiled type (even though they are, today) — a byte round-trip stays
/// correct even if a future dependency bump ever splits them, and is trivial
/// for a reviewer to audit for a money-critical path.
fn to_cashu_pk(pk: &PublicKey) -> Result<cashu::PublicKey, Nip60Error> {
    cashu::PublicKey::from_slice(&pk.serialize())
        .map_err(|e| Nip60Error::Crypto(format!("pubkey → cashu: {e}")))
}

/// The reverse of [`to_cashu_pk`].
fn from_cashu_pk(pk: &cashu::PublicKey) -> Result<PublicKey, Nip60Error> {
    PublicKey::from_slice(&pk.to_bytes())
        .map_err(|e| Nip60Error::Crypto(format!("pubkey ← cashu: {e}")))
}

/// Convert a workspace [`SecretKey`] to the `cashu` crate's newtype.
fn to_cashu_sk(sk: &SecretKey) -> Result<cashu::SecretKey, Nip60Error> {
    cashu::SecretKey::from_slice(&sk.secret_bytes())
        .map_err(|e| Nip60Error::Crypto(format!("secret key → cashu: {e}")))
}

/// Cashu's domain-separated hash-to-curve (NUT-00), via `cashu::dhke`.
pub fn hash_to_curve(message: &[u8]) -> Result<PublicKey, Nip60Error> {
    let y = cashu::dhke::hash_to_curve(message)
        .map_err(|e| Nip60Error::Crypto(format!("hash_to_curve: {e}")))?;
    from_cashu_pk(&y)
}

/// Produce a blinded message `B' = Y + r*G` from secret `x` and blinding factor `r`.
///
/// `_secp` is unused by this `cashu`-crate-backed implementation (`cashu`
/// manages its own secp256k1 context internally) — kept only so this
/// function's signature doesn't force a diff through the mint-HTTP lane
/// (`cashu/http/blinded.rs`, `cashu/http/mint.rs`, `cashu/http/swap.rs`,
/// `cashu/client.rs` all thread a shared `Secp256k1<All>` context down to
/// this call); that lane's shape is intentionally out of scope for this swap.
pub fn blind_message(
    secret_x: &[u8],
    blinding_factor: &SecretKey,
    _secp: &Secp256k1<All>,
) -> Result<PublicKey, Nip60Error> {
    let r = to_cashu_sk(blinding_factor)?;
    let (b_prime, _r_out) = cashu::dhke::blind_message(secret_x, Some(r))
        .map_err(|e| Nip60Error::Crypto(format!("blind_message: {e}")))?;
    from_cashu_pk(&b_prime)
}

/// Unblind a blind signature: `C = C' - r * K`.
///
/// `_secp` is unused — see [`blind_message`]'s doc comment.
pub fn unblind_signature(
    c_prime: &PublicKey,
    blinding_factor: &SecretKey,
    mint_pubkey: &PublicKey,
    _secp: &Secp256k1<All>,
) -> Result<PublicKey, Nip60Error> {
    let c_prime_c = to_cashu_pk(c_prime)?;
    let r = to_cashu_sk(blinding_factor)?;
    let mint_pk_c = to_cashu_pk(mint_pubkey)?;
    let c = cashu::dhke::unblind_message(&c_prime_c, &r, &mint_pk_c)
        .map_err(|e| Nip60Error::Crypto(format!("unblind_message: {e}")))?;
    from_cashu_pk(&c)
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
/// Routes through `cashu::nuts::nut00::BlindSignature::verify_dleq`, which
/// itself fails closed (`Err(MissingDleqProof)`) when a `BlindSignature`
/// carries no DLEQ proof — see the module doc comment for why that can't
/// happen from this function (it always attaches `proof`).
///
/// `amount` and `keyset_id` should be the mint response's real values — they
/// are required to construct a `cashu::BlindSignature` (the type the `cashu`
/// crate's verify method is defined on), though the NUT-12 verification math
/// itself only uses `c_prime`, the DLEQ scalars, and `mint_pubkey` (confirmed
/// against the `cashu` crate's `nut12` source: `amount`/`keyset_id` are
/// structural fields the check never reads).
#[allow(clippy::too_many_arguments)]
pub fn verify_dleq(
    proof: &DleqProof,
    b_prime: &PublicKey,
    c_prime: &PublicKey,
    mint_pubkey: &PublicKey,
    amount: u64,
    keyset_id: &str,
    _secp: &Secp256k1<All>,
) -> Result<(), Nip60Error> {
    let keyset_id = cashu::Id::try_from(keyset_id.to_string())
        .map_err(|e| Nip60Error::Crypto(format!("DLEQ verify: bad keyset id: {e}")))?;
    let e = cashu::SecretKey::from_slice(&proof.e)
        .map_err(|e| Nip60Error::Crypto(format!("DLEQ e as scalar: {e}")))?;
    let s = cashu::SecretKey::from_slice(&proof.s)
        .map_err(|e| Nip60Error::Crypto(format!("DLEQ s as scalar: {e}")))?;
    let c_prime_c = to_cashu_pk(c_prime)?;
    let b_prime_c = to_cashu_pk(b_prime)?;
    let mint_pk_c = to_cashu_pk(mint_pubkey)?;

    let blind_sig = cashu::BlindSignature {
        amount: cashu::Amount::from(amount),
        keyset_id,
        c: c_prime_c,
        dleq: Some(cashu::BlindSignatureDleq { e, s }),
    };

    blind_sig
        .verify_dleq(mint_pk_c, b_prime_c)
        .map_err(|e| Nip60Error::Crypto(format!("DLEQ proof invalid: {e}")))
}

/// The NUT-12 DLEQ challenge transcript hash `e = H(R1 || R2 || K || C')`.
///
/// Cashu (nutshell reference): SHA256 of the UTF-8 hex of UNCOMPRESSED
/// points, concatenated in that order. Nutshell uses uncompressed (65-byte,
/// `04||x||y`) points, NOT compressed (33-byte) ones, and the hash input is
/// the concatenated hex string encoded as UTF-8 bytes.
///
/// **Test-only mint-side simulation.** Production DLEQ *verification* now runs
/// entirely inside the audited `cashu` crate (see [`verify_dleq`]), so this
/// transcript formula is no longer part of any verification path — it is kept
/// solely to let test fixtures *produce* a genuine mint-signed DLEQ proof
/// (this crate is never a mint outside tests). `#[cfg(test)] pub(crate)` so the
/// shared mint-side prover fixture
/// (`cashu::http::mint_http_support::prove_dleq`) and the nutzap/swap DLEQ
/// tests all compute one IDENTICAL challenge, rather than each carrying an
/// independent copy of the formula that could silently drift. The `cashu`
/// crate's own `nut12` verifier uses this exact same `hash_e` transcript
/// (confirmed against its source), so a proof this produces is what
/// `verify_dleq` accepts.
#[cfg(test)]
pub(crate) fn dleq_challenge(
    r1: &PublicKey,
    r2: &PublicKey,
    mint_pubkey: &PublicKey,
    c_prime: &PublicKey,
) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let e_str = format!(
        "{}{}{}{}",
        hex::encode(r1.serialize_uncompressed()),
        hex::encode(r2.serialize_uncompressed()),
        hex::encode(mint_pubkey.serialize_uncompressed()),
        hex::encode(c_prime.serialize_uncompressed()),
    );
    let mut h = Sha256::new();
    h.update(e_str.as_bytes());
    h.finalize().into()
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
    use nostr::secp256k1;
    use nostr::secp256k1::Secp256k1;

    #[test]
    fn hash_to_curve_returns_valid_point() {
        let pt = hash_to_curve(b"test message").expect("hash_to_curve must succeed");
        // Round-trip through compressed serialisation.
        let bytes = pt.serialize();
        let pt2 = PublicKey::from_slice(&bytes).expect("deser");
        assert_eq!(pt, pt2);
    }

    /// Official Cashu NUT-00 test vector (from the `nutshell` reference
    /// implementation and re-used verbatim by the `cashu` crate's own test
    /// suite) — pins this adapter to the spec, not just to itself.
    #[test]
    fn hash_to_curve_matches_nut00_test_vector() {
        let secret =
            hex::decode("0000000000000000000000000000000000000000000000000000000000000000")
                .unwrap();
        let y = hash_to_curve(&secret).expect("htc");
        let expected = PublicKey::from_slice(
            &hex::decode("024cce997d3b518f739663b757deaec95bcd9473c30a14ac2fd04023a739d1a725")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(y, expected);
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
        let k_scalar = secp256k1::Scalar::from(k);
        let c_prime = b_prime.mul_tweak(&secp, &k_scalar).expect("mint sign");

        // Client: unblind → C
        let c = unblind_signature(&c_prime, &r, &mint_pk, &secp).expect("unblind");

        // Verify: C should equal k * Y where Y = hash_to_curve(secret)
        let y = hash_to_curve(secret).expect("htc");
        let k_scalar2 = secp256k1::Scalar::from(k);
        let expected_c = y.mul_tweak(&secp, &k_scalar2).expect("k*Y");
        assert_eq!(c, expected_c, "unblinded C must equal k*Y");
    }

    /// The mint-side DLEQ proof math (`R1 = r*G`, `R2 = r*B'`,
    /// `e = H(R1||R2||K||C')`, `s = r + e*k`) still lives in this test module
    /// (it simulates a mint, which this crate never is) so we can prove the
    /// `cashu`-crate-backed [`verify_dleq`] accepts a well-formed proof.
    #[test]
    fn verify_dleq_accepts_well_formed_proof() {
        let secp = Secp256k1::new();
        let secret = b"nutzap test secret";
        let r = SecretKey::new(&mut nostr::secp256k1::rand::thread_rng());
        let k = SecretKey::new(&mut nostr::secp256k1::rand::thread_rng());
        let mint_pk = PublicKey::from_secret_key(&secp, &k);

        let b_prime = blind_message(secret, &r, &secp).expect("blind");
        let k_scalar = secp256k1::Scalar::from(k);
        let c_prime = b_prime.mul_tweak(&secp, &k_scalar).expect("mint sign");

        // Mint-side DLEQ proof construction (NUT-12), via the shared
        // `dleq_challenge` transcript helper so this test and the
        // `mint_http_support::prove_dleq` fixture never drift apart.
        let nonce = SecretKey::new(&mut nostr::secp256k1::rand::thread_rng());
        let r1 = PublicKey::from_secret_key(&secp, &nonce);
        let nonce_scalar = secp256k1::Scalar::from(nonce);
        let r2 = b_prime.mul_tweak(&secp, &nonce_scalar).expect("r2");
        let e: [u8; 32] = dleq_challenge(&r1, &r2, &mint_pk, &c_prime);
        let e_sk = SecretKey::from_slice(&e).expect("e as sk");
        let e_scalar = secp256k1::Scalar::from(e_sk);
        // s = nonce + e*k
        let e_k = k.mul_tweak(&e_scalar).expect("e*k");
        let s_sk = nonce.add_tweak(&secp256k1::Scalar::from(e_k)).expect("s");

        verify_dleq(
            &DleqProof {
                e,
                s: s_sk.secret_bytes(),
            },
            &b_prime,
            &c_prime,
            &mint_pk,
            42,
            "00deadbeefcafe00",
            &secp,
        )
        .expect("well-formed DLEQ proof must verify");
    }

    /// #2933: a tampered/invalid DLEQ proof must be rejected, not silently
    /// accepted — the money-safety review finding this swap must preserve.
    #[test]
    fn verify_dleq_rejects_tampered_proof() {
        let secp = Secp256k1::new();
        let secret = b"nutzap test secret";
        let r = SecretKey::new(&mut nostr::secp256k1::rand::thread_rng());
        let k = SecretKey::new(&mut nostr::secp256k1::rand::thread_rng());
        let mint_pk = PublicKey::from_secret_key(&secp, &k);

        let b_prime = blind_message(secret, &r, &secp).expect("blind");
        let k_scalar = secp256k1::Scalar::from(k);
        let c_prime = b_prime.mul_tweak(&secp, &k_scalar).expect("mint sign");

        // Garbage e/s — not a real DLEQ proof for this (B', C', K).
        let bogus = DleqProof {
            e: [0x11; 32],
            s: [0x22; 32],
        };

        let result = verify_dleq(
            &bogus,
            &b_prime,
            &c_prime,
            &mint_pk,
            42,
            "00deadbeefcafe00",
            &secp,
        );
        assert!(result.is_err(), "tampered DLEQ proof must be rejected");
    }
}
