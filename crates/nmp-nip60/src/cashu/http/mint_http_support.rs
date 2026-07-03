//! Test-only fixtures shared by `http`'s submodule test files
//! ([`super::quote`], [`super::mint`], [`super::swap`], [`super::checkstate`]).
//! `#[cfg(test)]`-only (see `http/mod.rs`); named `_support.rs` per the
//! repo's split-test-file convention, never bare `support.rs`.

use std::collections::HashMap;

use nostr::secp256k1::{All, PublicKey, Scalar, Secp256k1, SecretKey};

use super::MintRawResponse;
use crate::cashu::crypto::dleq_challenge;
use crate::cashu::types::KeySet;

pub(crate) fn secp() -> Secp256k1<All> {
    Secp256k1::new()
}

/// A fixture keyset with denominations `1..=128` (bits 0..8), all signed by
/// the same freshly-generated mint key `k`. Returns `(keyset, k)` so a test
/// can simulate the mint's signature over any prepared blinded output.
pub(crate) fn fixture_keyset() -> (KeySet, SecretKey) {
    let secp = secp();
    let k = SecretKey::new(&mut nostr::secp256k1::rand::thread_rng());
    let mint_pk = PublicKey::from_secret_key(&secp, &k);
    let mut keys = HashMap::new();
    for bit in 0..8u64 {
        keys.insert((1u64 << bit).to_string(), hex::encode(mint_pk.serialize()));
    }
    (
        KeySet {
            id: "00keyset".to_string(),
            unit: "sat".to_string(),
            keys,
            input_fee_ppk: 0,
        },
        k,
    )
}

/// A 200-OK [`MintRawResponse`] carrying `body` verbatim.
pub(crate) fn ok(body: &[u8]) -> MintRawResponse {
    MintRawResponse {
        status_code: 200,
        body: body.to_vec(),
    }
}

/// The mint-side prover for a genuine NUT-12 DLEQ proof over `c_prime = k *
/// b_prime` — the counterpart `crate::cashu::crypto::verify_dleq` checks.
/// Existing swap/mint tests only ever construct a bogus/absent DLEQ (see
/// `swap::tests::finalize_swap_rejects_tampered_dleq_proof`); this is for
/// tests that need the "DLEQ present AND actually valid" branch instead,
/// e.g. `nutzap::verify_nutzap_dleq_against_keyset`'s #2933 fail-closed test.
/// Returns `(e_hex, s_hex)`. Shares `crypto::dleq_challenge`'s transcript-hash
/// formula with `verify_dleq` itself — never a second, independently
/// maintained copy of that hash that could silently drift from it.
pub(crate) fn prove_dleq(
    b_prime: &PublicKey,
    c_prime: &PublicKey,
    mint_sk: &SecretKey,
    secp: &Secp256k1<All>,
) -> (String, String) {
    let mint_pk = PublicKey::from_secret_key(secp, mint_sk);
    let nonce = SecretKey::new(&mut nostr::secp256k1::rand::thread_rng());
    let r1 = PublicKey::from_secret_key(secp, &nonce);
    let r2 = b_prime
        .mul_tweak(secp, &Scalar::from(nonce))
        .expect("b_prime * nonce");

    let e_bytes = dleq_challenge(&r1, &r2, &mint_pk, c_prime);

    // s = nonce + e*k
    let e_scalar = Scalar::from(SecretKey::from_slice(&e_bytes).expect("e as scalar"));
    let e_k = mint_sk.mul_tweak(&e_scalar).expect("k*e");
    let s_sk = nonce.add_tweak(&Scalar::from(e_k)).expect("nonce + k*e");

    (hex::encode(e_bytes), hex::encode(s_sk.secret_bytes()))
}
