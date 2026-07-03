//! Test-only fixtures shared by `http`'s submodule test files
//! ([`super::quote`], [`super::mint`], [`super::swap`], [`super::checkstate`]).
//! `#[cfg(test)]`-only (see `http/mod.rs`); named `_support.rs` per the
//! repo's split-test-file convention, never bare `support.rs`.

use std::collections::HashMap;

use nostr::secp256k1::{All, PublicKey, Secp256k1, SecretKey};

use super::MintRawResponse;
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
