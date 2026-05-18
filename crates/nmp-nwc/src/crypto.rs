//! NIP-44 encrypt/decrypt helpers for the NWC client keypair.
//!
//! NWC uses a dedicated client keypair (not the user's identity key).
//! The `secret` in the NWC URI is the client's 64-char hex secret key;
//! all kind:23194 requests are signed with it and the content is NIP-44
//! encrypted to the wallet pubkey. Kind:23195 responses arrive encrypted
//! to the client pubkey and are decrypted with the client secret.

use nostr::nips::nip44;
use nostr::{Keys, PublicKey, SecretKey};

/// Encrypt `plaintext` from the NWC client to the wallet pubkey.
///
/// `client_secret_hex`: 64-char hex of the client's secret key (from the NWC URI).
/// `wallet_pubkey_hex`: 64-char hex of the wallet service pubkey (from the NWC URI).
pub fn encrypt(
    client_secret_hex: &str,
    wallet_pubkey_hex: &str,
    plaintext: &str,
) -> Result<String, String> {
    let sk = parse_secret(client_secret_hex)?;
    let pk = parse_pubkey(wallet_pubkey_hex)?;
    nip44::encrypt(&sk, &pk, plaintext, nip44::Version::V2)
        .map_err(|e| format!("nip44 encrypt: {e}"))
}

/// Decrypt a kind:23195 response from the wallet pubkey to the NWC client.
///
/// `client_secret_hex`: 64-char hex of the client's secret key.
/// `wallet_pubkey_hex`: 64-char hex of the wallet pubkey (event author).
pub fn decrypt(
    client_secret_hex: &str,
    wallet_pubkey_hex: &str,
    payload: &str,
) -> Result<String, String> {
    let sk = parse_secret(client_secret_hex)?;
    let pk = parse_pubkey(wallet_pubkey_hex)?;
    nip44::decrypt(&sk, &pk, payload).map_err(|e| format!("nip44 decrypt: {e}"))
}

/// Derive the client public key from the client secret hex.
pub fn client_pubkey_hex(client_secret_hex: &str) -> Result<String, String> {
    let sk = parse_secret(client_secret_hex)?;
    let keys = Keys::new(sk);
    Ok(keys.public_key().to_hex())
}

fn parse_secret(hex: &str) -> Result<SecretKey, String> {
    SecretKey::from_hex(hex).map_err(|e| format!("invalid client secret: {e}"))
}

fn parse_pubkey(hex: &str) -> Result<PublicKey, String> {
    PublicKey::from_hex(hex).map_err(|e| format!("invalid wallet pubkey: {e}"))
}
