//! HTTP client for the Cashu mint API (NUT-01 through NUT-12).
//!
//! All calls are synchronous (uses `ureq`), matching the pattern used by
//! `nmp-nip57`'s LNURL fetcher. The caller is responsible for spawning
//! these on a worker thread (D8 — never block the actor loop).

use nostr::secp256k1::{Secp256k1, SecretKey};
use std::collections::HashMap;
use tracing::debug;

use super::crypto::{blind_message, hash_to_curve, unblind_signature, verify_dleq, DleqProof};
use super::types::*;
use crate::error::Nip60Error;

/// Cashu mint HTTP client.
///
/// Holds the mint URL and a cached keyset. Constructed once per mint URL
/// and reused across operations.
pub struct MintClient {
    mint_url: String,
    secp: Secp256k1<nostr::secp256k1::All>,
}

impl MintClient {
    /// Create a new client for the given mint URL.
    pub fn new(mint_url: impl Into<String>) -> Self {
        Self {
            mint_url: mint_url.into().trim_end_matches('/').to_string(),
            secp: Secp256k1::new(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.mint_url, path)
    }

    fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, Nip60Error> {
        let url = self.url(path);
        debug!("GET {url}");
        ureq::get(&url)
            .call()
            .map_err(|e| Nip60Error::MintHttp(format!("{e}")))?
            .into_json::<T>()
            .map_err(|e| Nip60Error::MintHttp(format!("decode: {e}")))
    }

    fn post<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, Nip60Error> {
        let url = self.url(path);
        debug!("POST {url}");
        ureq::post(&url)
            .set("Content-Type", "application/json")
            .send_json(body)
            .map_err(|e| match e {
                ureq::Error::Status(_, resp) => {
                    let text = resp.into_string().unwrap_or_default();
                    Nip60Error::MintProtocol(text)
                }
                other => Nip60Error::MintHttp(format!("{other}")),
            })?
            .into_json::<T>()
            .map_err(|e| Nip60Error::MintHttp(format!("decode: {e}")))
    }

    // ─── Keyset ────────────────────────────────────────────────────────────

    /// Fetch the mint's active keysets.
    pub fn get_keys(&self) -> Result<KeysResponse, Nip60Error> {
        self.get("/v1/keys")
    }

    /// Fetch the active sat-unit keyset (with fee info from /v1/keysets merged in).
    pub fn get_sat_keyset(&self) -> Result<KeySet, Nip60Error> {
        // /v1/keys has the denomination→pubkey map; /v1/keysets has input_fee_ppk.
        let keys_resp = self.get_keys()?;
        let mut keyset = keys_resp
            .keysets
            .into_iter()
            .find(|ks| ks.unit == "sat")
            .ok_or_else(|| Nip60Error::MintProtocol("no sat keyset found".into()))?;

        // Merge in fee info from /v1/keysets (best-effort; ignore errors).
        if let Ok(keysets_resp) = self.get::<KeysResponse>("/v1/keysets") {
            if let Some(ks_meta) = keysets_resp.keysets.into_iter().find(|ks| ks.id == keyset.id) {
                keyset.input_fee_ppk = ks_meta.input_fee_ppk;
            }
        }
        Ok(keyset)
    }

    /// Compute the swap fee for `n` inputs given `input_fee_ppk` (parts per thousand).
    ///
    /// Per NUT-02: `fee = ceil(n * input_fee_ppk / 1000)`.
    pub fn compute_fee(n_inputs: u64, input_fee_ppk: u64) -> u64 {
        (n_inputs * input_fee_ppk).div_ceil(1000)
    }

    // ─── Mint quote (NUT-04) ──────────────────────────────────────────────

    /// Request a bolt11 mint quote for the given amount in sats.
    pub fn create_mint_quote(&self, amount_sats: u64) -> Result<MintQuoteResponse, Nip60Error> {
        self.post(
            "/v1/mint/quote/bolt11",
            &MintQuoteRequest {
                amount: amount_sats,
                unit: "sat".into(),
            },
        )
    }

    /// Poll the status of an existing mint quote.
    pub fn get_mint_quote_status(&self, quote_id: &str) -> Result<MintQuoteResponse, Nip60Error> {
        self.get(&format!("/v1/mint/quote/bolt11/{quote_id}"))
    }

    // ─── Mint tokens (NUT-04) ─────────────────────────────────────────────

    /// Mint tokens for a paid quote.
    ///
    /// Returns a list of `Proof`s whose amounts sum to `total_amount` using
    /// the standard 2^n denomination split.
    ///
    /// Each proof is verified with its DLEQ proof if the mint provides one
    /// (NUT-12). Returns an error if any DLEQ verification fails.
    pub fn mint_tokens(
        &self,
        quote_id: &str,
        total_amount: u64,
        keyset: &KeySet,
    ) -> Result<Vec<Proof>, Nip60Error> {
        let denominations = split_amount(total_amount);
        let mint_pubkey_for_amount = build_pubkey_map(keyset)?;

        // Generate one blinded message per denomination.
        let mut secrets: Vec<String> = Vec::with_capacity(denominations.len());
        let mut blinding_factors: Vec<SecretKey> = Vec::with_capacity(denominations.len());
        let mut b_primes: Vec<String> = Vec::with_capacity(denominations.len());

        for &_amount in &denominations {
            // The Cashu secret is a UTF-8 string; hash_to_curve must be called on
            // its UTF-8 bytes so the mint can reproduce Y = hash_to_curve(secret.as_bytes()).
            // We generate random bytes, hex-encode them as the canonical secret string,
            // then pass the UTF-8 bytes of that hex string to blind_message.
            let secret_raw = crate::cashu::crypto::random_secret();
            let secret_hex = hex::encode(secret_raw);
            let r = SecretKey::new(&mut nostr::secp256k1::rand::thread_rng());
            let b_prime = blind_message(secret_hex.as_bytes(), &r, &self.secp)?;
            secrets.push(secret_hex);
            blinding_factors.push(r);
            b_primes.push(hex::encode(b_prime.serialize()));
        }

        let outputs: Vec<BlindedMessage> = denominations
            .iter()
            .zip(b_primes.iter())
            .map(|(&amount, b_prime)| BlindedMessage {
                amount,
                id: keyset.id.clone(),
                b_prime: b_prime.clone(),
            })
            .collect();

        let resp: MintTokensResponse = self.post(
            "/v1/mint/bolt11",
            &MintTokensRequest {
                quote: quote_id.to_string(),
                outputs,
            },
        )?;

        // Unblind signatures and verify DLEQ proofs.
        let mut proofs = Vec::with_capacity(denominations.len());
        for (i, sig) in resp.signatures.iter().enumerate() {
            let amount = sig.amount;
            let mint_pk = mint_pubkey_for_amount.get(&amount).ok_or_else(|| {
                Nip60Error::Crypto(format!("no mint pubkey for amount {amount}"))
            })?;

            let c_prime_bytes = hex::decode(&sig.c_prime)
                .map_err(|e| Nip60Error::Crypto(format!("C' decode: {e}")))?;
            let c_prime = nostr::secp256k1::PublicKey::from_slice(&c_prime_bytes)
                .map_err(|e| Nip60Error::Crypto(format!("C' parse: {e}")))?;

            // Verify DLEQ proof if present.
            if let Some(dleq_wire) = &sig.dleq {
                let b_prime_bytes = hex::decode(&b_primes[i])
                    .map_err(|e| Nip60Error::Crypto(format!("B' decode: {e}")))?;
                let b_prime_pt = nostr::secp256k1::PublicKey::from_slice(&b_prime_bytes)
                    .map_err(|e| Nip60Error::Crypto(format!("B' parse: {e}")))?;
                let dleq = wire_to_dleq(dleq_wire)?;
                verify_dleq(&dleq, &b_prime_pt, &c_prime, mint_pk, &self.secp)?;
                debug!("DLEQ proof verified for amount {amount}");
            }

            let c = unblind_signature(&c_prime, &blinding_factors[i], mint_pk, &self.secp)?;

            // Include the blinding factor r in the DLEQ proof so the mint can
            // re-verify when this proof is spent as an input to a future swap.
            let dleq_with_r = sig.dleq.as_ref().map(|d| DleqProofWire {
                e: d.e.clone(),
                s: d.s.clone(),
                r: Some(hex::encode(blinding_factors[i].secret_bytes())),
            });

            proofs.push(Proof {
                amount,
                id: keyset.id.clone(),
                secret: secrets[i].clone(),
                c: hex::encode(c.serialize()),
                dleq: dleq_with_r,
                witness: None,
            });
        }

        Ok(proofs)
    }

    // ─── Swap (NUT-03) ────────────────────────────────────────────────────

    /// Swap proofs for new proofs, optionally with P2PK spending conditions.
    ///
    /// `new_secrets` — if `Some`, these are the secrets for the output proofs
    /// (used for P2PK where the secret is a spending condition JSON).
    /// If `None`, random secrets are generated.
    pub fn swap(
        &self,
        inputs: Vec<Proof>,
        output_amounts: Vec<u64>,
        output_secrets: Option<Vec<String>>,
        keyset: &KeySet,
    ) -> Result<Vec<Proof>, Nip60Error> {
        let mint_pubkey_for_amount = build_pubkey_map(keyset)?;
        let n = output_amounts.len();

        let mut raw_secrets: Vec<Vec<u8>> = Vec::with_capacity(n);
        let mut blinding_factors: Vec<SecretKey> = Vec::with_capacity(n);
        let mut b_primes_hex: Vec<String> = Vec::with_capacity(n);

        for (i, _) in output_amounts.iter().enumerate() {
            // Secrets are UTF-8 strings; hash_to_curve uses their UTF-8 bytes.
            // Random secrets are hex-encoded (same pattern as mint_tokens).
            let secret_str = if let Some(ref secrets) = output_secrets {
                secrets[i].clone()
            } else {
                hex::encode(crate::cashu::crypto::random_secret())
            };
            let r = SecretKey::new(&mut nostr::secp256k1::rand::thread_rng());
            let b_prime = blind_message(secret_str.as_bytes(), &r, &self.secp)?;
            raw_secrets.push(secret_str.into_bytes());
            blinding_factors.push(r);
            b_primes_hex.push(hex::encode(b_prime.serialize()));
        }

        let swap_outputs: Vec<BlindedMessage> = output_amounts
            .iter()
            .zip(b_primes_hex.iter())
            .map(|(&amount, b_prime)| BlindedMessage {
                amount,
                id: keyset.id.clone(),
                b_prime: b_prime.clone(),
            })
            .collect();

        let resp: SwapResponse = self.post(
            "/v1/swap",
            &SwapRequest {
                inputs,
                outputs: swap_outputs,
            },
        )?;

        let mut new_proofs = Vec::with_capacity(n);
        for (i, sig) in resp.signatures.iter().enumerate() {
            let amount = sig.amount;
            let mint_pk = mint_pubkey_for_amount.get(&amount).ok_or_else(|| {
                Nip60Error::Crypto(format!("no mint pubkey for amount {amount}"))
            })?;

            let c_prime_bytes = hex::decode(&sig.c_prime)
                .map_err(|e| Nip60Error::Crypto(format!("swap C' decode: {e}")))?;
            let c_prime = nostr::secp256k1::PublicKey::from_slice(&c_prime_bytes)
                .map_err(|e| Nip60Error::Crypto(format!("swap C' parse: {e}")))?;

            // Verify DLEQ proof if present.
            if let Some(dleq_wire) = &sig.dleq {
                let b_prime_bytes = hex::decode(&b_primes_hex[i])
                    .map_err(|e| Nip60Error::Crypto(format!("swap B' decode: {e}")))?;
                let b_prime_pt = nostr::secp256k1::PublicKey::from_slice(&b_prime_bytes)
                    .map_err(|e| Nip60Error::Crypto(format!("swap B' parse: {e}")))?;
                let dleq = wire_to_dleq(dleq_wire)?;
                verify_dleq(&dleq, &b_prime_pt, &c_prime, mint_pk, &self.secp)?;
                debug!("DLEQ proof verified in swap for amount {amount}");
            }

            let c = unblind_signature(&c_prime, &blinding_factors[i], mint_pk, &self.secp)?;
            let secret_str = if output_secrets.is_some() {
                String::from_utf8(raw_secrets[i].clone())
                    .map_err(|e| Nip60Error::Crypto(format!("secret utf8: {e}")))?
            } else {
                hex::encode(&raw_secrets[i])
            };

            let dleq_with_r = sig.dleq.as_ref().map(|d| DleqProofWire {
                e: d.e.clone(),
                s: d.s.clone(),
                r: Some(hex::encode(blinding_factors[i].secret_bytes())),
            });

            new_proofs.push(Proof {
                amount,
                id: keyset.id.clone(),
                secret: secret_str,
                c: hex::encode(c.serialize()),
                dleq: dleq_with_r,
                witness: None,
            });
        }
        Ok(new_proofs)
    }

    // ─── Proof state check (NUT-07) ───────────────────────────────────────

    /// Check which of the given proof secrets are still unspent.
    pub fn check_state(&self, secrets: &[String]) -> Result<Vec<ProofState>, Nip60Error> {
        let ys: Result<Vec<String>, _> = secrets
            .iter()
            .map(|s| {
                let bytes = if s.starts_with("{") {
                    s.as_bytes().to_vec()
                } else {
                    hex::decode(s).map_err(|e| Nip60Error::Crypto(format!("secret hex: {e}")))?
                };
                let pt = hash_to_curve(&bytes)?;
                Ok::<String, Nip60Error>(hex::encode(pt.serialize()))
            })
            .collect();
        let resp: StateCheckResponse = self.post("/v1/checkstate", &StateCheckRequest { ys: ys? })?;
        Ok(resp.states)
    }
}

// ─── Helpers ───────────────────────────────────────────────────────────────

/// Split `amount` into powers-of-2 denominations (smallest first).
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

/// Build a map from denomination to mint public key from a keyset.
pub fn build_pubkey_map(
    keyset: &KeySet,
) -> Result<HashMap<u64, nostr::secp256k1::PublicKey>, Nip60Error> {
    let mut map = HashMap::new();
    for (amount_str, pubkey_hex) in &keyset.keys {
        let amount: u64 = amount_str
            .parse()
            .map_err(|_| Nip60Error::Crypto(format!("invalid amount key: {amount_str}")))?;
        let bytes =
            hex::decode(pubkey_hex).map_err(|e| Nip60Error::Crypto(format!("keyset pk: {e}")))?;
        let pk = nostr::secp256k1::PublicKey::from_slice(&bytes)
            .map_err(|e| Nip60Error::Crypto(format!("keyset pk parse: {e}")))?;
        map.insert(amount, pk);
    }
    Ok(map)
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

    #[test]
    fn split_amount_basic() {
        assert_eq!(split_amount(64), vec![64]);
        assert_eq!(split_amount(3), vec![1, 2]);
        assert_eq!(split_amount(7), vec![1, 2, 4]);
    }

    #[test]
    fn split_amount_zero() {
        assert_eq!(split_amount(0), Vec::<u64>::new());
    }
}
