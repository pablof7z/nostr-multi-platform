//! NIP-04 / NIP-44 encrypt/decrypt helpers for the NWC client keypair.
//!
//! NWC uses a dedicated client keypair (not the user's identity key).
//! The `secret` in the NWC URI is the client's 64-char hex secret key;
//! all kind:23194 requests are signed with it and the content is encrypted
//! to the wallet pubkey. Kind:23195 responses arrive encrypted to the client
//! pubkey and are decrypted with the client secret.
//!
//! **Encryption flavor**: NIP-47 historically uses NIP-04 (the AES-CBC scheme).
//! NIP-44 v2 support is newer and not all wallet services accept it. We
//! default to NIP-04 for maximum compatibility; decryption tries NIP-44 first
//! (cheap detection on the `?iv=` marker) and falls back to NIP-04.

use nostr::nips::{nip04, nip44};
use nostr::{Keys, PublicKey, SecretKey};

/// Encrypt `plaintext` from the NWC client to the wallet pubkey using NIP-04.
/// This is the historical NIP-47 default and the only flavor universally
/// supported across wallet implementations (Alby, Mutiny, Zeus, etc.).
pub fn encrypt(
    client_secret_hex: &str,
    wallet_pubkey_hex: &str,
    plaintext: &str,
) -> Result<String, String> {
    let sk = parse_secret(client_secret_hex)?;
    let pk = parse_pubkey(wallet_pubkey_hex)?;
    nip04::encrypt(&sk, &pk, plaintext).map_err(|e| format!("nip04 encrypt: {e}"))
}

/// Decrypt a kind:23195 response from the wallet pubkey to the NWC client.
///
/// Tries NIP-04 first (the historical NIP-47 default — payload contains `?iv=`).
/// Falls back to NIP-44 v2 for newer wallets that opt into it.
pub fn decrypt(
    client_secret_hex: &str,
    wallet_pubkey_hex: &str,
    payload: &str,
) -> Result<String, String> {
    let sk = parse_secret(client_secret_hex)?;
    let pk = parse_pubkey(wallet_pubkey_hex)?;
    // NIP-04 payloads carry an `?iv=` query-string suffix; NIP-44 v2 payloads
    // are pure base64. Use the marker as a cheap discriminator.
    if payload.contains("?iv=") {
        nip04::decrypt(&sk, &pk, payload).map_err(|e| format!("nip04 decrypt: {e}"))
    } else {
        nip44::decrypt(&sk, &pk, payload).map_err(|e| format!("nip44 decrypt: {e}"))
    }
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
