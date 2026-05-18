//! `nmp-nip44` — NIP-44 v2 payload encryption.
//!
//! Implements the NIP-44 v2 spec:
//! <https://github.com/nostr-protocol/nips/blob/master/44.md>
//!
//! # Algorithm summary
//! 1. ECDH on secp256k1 → shared point X coordinate (32 bytes, raw IKM).
//! 2. HKDF-Extract(SHA-256, ikm=ecdh_x, salt="nip44-v2") → conversation_key (32 bytes).
//! 3. Per-message: 32-byte random nonce → HKDF-Expand(conversation_key, info=nonce, 76 bytes)
//!    → chacha_key (32) || chacha_nonce (12) || hmac_key (32).
//! 4. Plaintext is UTF-8; length-prefixed and padded to a power-of-two bucket (min 32).
//! 5. ChaCha20 (stream cipher, no auth tag) encrypts the padded plaintext.
//! 6. MAC = HMAC-SHA256(hmac_key, nonce || ciphertext).
//! 7. Payload = base64(0x02 || nonce (32) || ciphertext || mac (32)).

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chacha20::cipher::{KeyIvInit, StreamCipher};
use chacha20::ChaCha20;
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand_core::{OsRng, RngCore};
use secp256k1::ecdh::shared_secret_point;
use secp256k1::{PublicKey, SecretKey};
use sha2::Sha256;
use thiserror::Error;

type HmacSha256 = Hmac<Sha256>;

/// Errors returned by NIP-44 operations.
#[derive(Debug, Error)]
pub enum Nip44Error {
    #[error("invalid secret key: {0}")]
    InvalidSecretKey(String),
    #[error("invalid public key: {0}")]
    InvalidPublicKey(String),
    #[error("plaintext length {0} is out of range (must be 1..=65535)")]
    PlaintextTooLong(usize),
    #[error("plaintext must not be empty")]
    PlaintextEmpty,
    #[error("base64 decode error: {0}")]
    Base64Decode(String),
    #[error("payload too short (got {0} bytes, need at least 99)")]
    PayloadTooShort(usize),
    #[error("unknown encryption version: expected 0x02, got 0x{0:02x}")]
    UnknownVersion(u8),
    #[error("HMAC verification failed (invalid MAC)")]
    InvalidMac,
    #[error("invalid padding in decrypted plaintext")]
    InvalidPadding,
}

/// The NIP-44 v2 conversation key derived from an ECDH shared secret.
///
/// Reuse this across multiple messages between the same two parties to avoid
/// re-running ECDH for every message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationKey([u8; 32]);

impl ConversationKey {
    /// Derive a conversation key from a sender/recipient secret key and the other party's
    /// public key. The result is symmetric: derive(sk1, pk2) == derive(sk2, pk1).
    ///
    /// Uses `shared_secret_point` (raw X‖Y, 64 bytes) and takes the X coordinate as IKM
    /// for HKDF-Extract(salt="nip44-v2", ikm=ecdh_x) → PRK = conversation_key.
    pub fn derive(secret_key: &SecretKey, public_key: &PublicKey) -> Self {
        // shared_secret_point returns [u8; 64]: X (32 bytes) || Y (32 bytes)
        let point = shared_secret_point(public_key, secret_key);
        let ecdh_x = &point[..32];

        // HKDF-Extract(salt="nip44-v2", ikm=ecdh_x) → PRK (32 bytes) = conversation_key
        let (prk, _) = Hkdf::<Sha256>::extract(Some(b"nip44-v2"), ecdh_x);
        ConversationKey(prk.into())
    }

    /// Construct from raw 32-byte conversation key (used with test vectors).
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        ConversationKey(bytes)
    }

    /// Return the raw conversation key bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Derive per-message keys from a 32-byte nonce using HKDF-Expand.
    ///
    /// Returns `(chacha_key [32], chacha_nonce [12], hmac_key [32])`.
    ///
    /// Spec: HKDF-Expand(PRK=conversation_key, info=nonce, L=76).
    pub fn message_keys(&self, nonce: &[u8; 32]) -> ([u8; 32], [u8; 12], [u8; 32]) {
        let hk = Hkdf::<Sha256>::from_prk(&self.0)
            .expect("conversation key is a valid 32-byte PRK");
        let mut okm = [0u8; 76];
        hk.expand(nonce, &mut okm)
            .expect("76 bytes is a valid HKDF-Expand output length");

        let mut chacha_key = [0u8; 32];
        let mut chacha_nonce = [0u8; 12];
        let mut hmac_key = [0u8; 32];
        chacha_key.copy_from_slice(&okm[0..32]);
        chacha_nonce.copy_from_slice(&okm[32..44]);
        hmac_key.copy_from_slice(&okm[44..76]);
        (chacha_key, chacha_nonce, hmac_key)
    }
}

/// Compute the padded length for a plaintext of `unpadded_len` bytes.
///
/// Per the NIP-44 spec's padding function:
/// - If unpadded_len <= 32 → 32
/// - Otherwise:
///   - next_power = 1 << (floor(log2(unpadded_len - 1)) + 1)  [smallest power of 2 > unpadded_len-1]
///   - chunk = max(next_power / 8, 32)
///   - padded = ceil(unpadded_len / chunk) * chunk
pub fn calc_padded_len(unpadded_len: usize) -> usize {
    if unpadded_len <= 32 {
        return 32;
    }
    // floor(log2(n-1)) = position of the highest set bit of (n-1)
    let bits = usize::BITS as usize - (unpadded_len - 1).leading_zeros() as usize;
    let next_pow2: usize = 1 << bits;
    let chunk = (next_pow2 / 8).max(32);
    // ceil(unpadded_len / chunk) * chunk  ==  ((unpadded_len - 1) / chunk + 1) * chunk
    ((unpadded_len - 1) / chunk + 1) * chunk
}

/// Encrypt with a provided conversation key and nonce (deterministic, for testing).
///
/// Returns the base64-encoded NIP-44 v2 payload.
pub fn encrypt_with_conversation_key(
    conversation_key: &ConversationKey,
    nonce: &[u8; 32],
    plaintext: &str,
) -> Result<String, Nip44Error> {
    let pt_bytes = plaintext.as_bytes();
    if pt_bytes.is_empty() {
        return Err(Nip44Error::PlaintextEmpty);
    }
    if pt_bytes.len() > 65535 {
        return Err(Nip44Error::PlaintextTooLong(pt_bytes.len()));
    }

    let padded_len = calc_padded_len(pt_bytes.len());
    // 2-byte big-endian length prefix + padded zero-filled content
    let mut padded = vec![0u8; 2 + padded_len];
    let len16 = pt_bytes.len() as u16;
    padded[0] = (len16 >> 8) as u8;
    padded[1] = (len16 & 0xff) as u8;
    padded[2..2 + pt_bytes.len()].copy_from_slice(pt_bytes);
    // remaining bytes are 0x00 padding

    let (chacha_key, chacha_nonce, hmac_key) = conversation_key.message_keys(nonce);

    // ChaCha20 encrypt in-place
    let mut ciphertext = padded;
    let mut cipher = ChaCha20::new((&chacha_key).into(), (&chacha_nonce).into());
    cipher.apply_keystream(&mut ciphertext);

    // MAC = HMAC-SHA256(hmac_key, nonce || ciphertext)
    let mut mac = HmacSha256::new_from_slice(&hmac_key)
        .expect("HMAC accepts any key length");
    mac.update(nonce);
    mac.update(&ciphertext);
    let tag = mac.finalize().into_bytes();

    // Payload: 0x02 || nonce (32) || ciphertext (2+padded_len) || mac (32)
    let mut payload = Vec::with_capacity(1 + 32 + ciphertext.len() + 32);
    payload.push(0x02u8);
    payload.extend_from_slice(nonce);
    payload.extend_from_slice(&ciphertext);
    payload.extend_from_slice(&tag);

    Ok(BASE64.encode(&payload))
}

/// Decrypt with a provided conversation key (for testing or when conv key is pre-derived).
pub fn decrypt_with_conversation_key(
    conversation_key: &ConversationKey,
    payload: &str,
) -> Result<String, Nip44Error> {
    let raw = BASE64
        .decode(payload.trim())
        .map_err(|e| Nip44Error::Base64Decode(e.to_string()))?;

    // Minimum valid payload: 1 (version) + 32 (nonce) + 2+32 (min padded msg) + 32 (mac) = 99
    if raw.len() < 99 {
        return Err(Nip44Error::PayloadTooShort(raw.len()));
    }

    let version = raw[0];
    if version != 0x02 {
        return Err(Nip44Error::UnknownVersion(version));
    }

    let nonce: &[u8; 32] = raw[1..33].try_into().unwrap();
    let ciphertext = &raw[33..raw.len() - 32];
    let mac_bytes = &raw[raw.len() - 32..];

    let (chacha_key, chacha_nonce, hmac_key) = conversation_key.message_keys(nonce);

    // Verify HMAC-SHA256(hmac_key, nonce || ciphertext)
    let mut mac = HmacSha256::new_from_slice(&hmac_key)
        .expect("HMAC accepts any key length");
    mac.update(nonce);
    mac.update(ciphertext);
    mac.verify_slice(mac_bytes)
        .map_err(|_| Nip44Error::InvalidMac)?;

    // ChaCha20 decrypt
    let mut plaintext_padded = ciphertext.to_vec();
    let mut cipher = ChaCha20::new((&chacha_key).into(), (&chacha_nonce).into());
    cipher.apply_keystream(&mut plaintext_padded);

    // Unpad: first 2 bytes are big-endian length of original plaintext
    if plaintext_padded.len() < 2 {
        return Err(Nip44Error::InvalidPadding);
    }
    let pt_len = ((plaintext_padded[0] as usize) << 8) | (plaintext_padded[1] as usize);

    if pt_len == 0 {
        return Err(Nip44Error::InvalidPadding);
    }
    if 2 + pt_len > plaintext_padded.len() {
        return Err(Nip44Error::InvalidPadding);
    }

    // Validate: all padding bytes (after plaintext) must be 0x00
    for &b in &plaintext_padded[2 + pt_len..] {
        if b != 0 {
            return Err(Nip44Error::InvalidPadding);
        }
    }

    // Validate total padded length matches the spec's padding function
    let expected_padded_len = calc_padded_len(pt_len);
    if plaintext_padded.len() != 2 + expected_padded_len {
        return Err(Nip44Error::InvalidPadding);
    }

    let plaintext_bytes = &plaintext_padded[2..2 + pt_len];
    String::from_utf8(plaintext_bytes.to_vec())
        .map_err(|_| Nip44Error::InvalidPadding)
}

/// Encrypt a plaintext from sender to recipient using a random nonce.
///
/// Returns a base64-encoded NIP-44 v2 payload.
pub fn encrypt(
    sender_sk: &SecretKey,
    recipient_pk: &PublicKey,
    plaintext: &str,
) -> Result<String, Nip44Error> {
    let conv_key = ConversationKey::derive(sender_sk, recipient_pk);
    let mut nonce = [0u8; 32];
    OsRng.fill_bytes(&mut nonce);
    encrypt_with_conversation_key(&conv_key, &nonce, plaintext)
}

/// Decrypt a NIP-44 v2 payload for the recipient.
pub fn decrypt(
    recipient_sk: &SecretKey,
    sender_pk: &PublicKey,
    payload: &str,
) -> Result<String, Nip44Error> {
    let conv_key = ConversationKey::derive(recipient_sk, sender_pk);
    decrypt_with_conversation_key(&conv_key, payload)
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_calc_padded_len_spec_examples() {
        // From NIP-44 spec test vector pairs
        assert_eq!(calc_padded_len(1), 32);
        assert_eq!(calc_padded_len(16), 32);
        assert_eq!(calc_padded_len(32), 32);
        assert_eq!(calc_padded_len(33), 64);
        assert_eq!(calc_padded_len(37), 64);
        assert_eq!(calc_padded_len(45), 64);
        assert_eq!(calc_padded_len(65), 96);
        assert_eq!(calc_padded_len(100), 128);
        assert_eq!(calc_padded_len(200), 224);
        assert_eq!(calc_padded_len(515), 640);
        assert_eq!(calc_padded_len(65536), 65536);
    }

    #[test]
    fn test_roundtrip() {
        use secp256k1::Secp256k1;
        let secp = Secp256k1::new();
        let sk1 = SecretKey::from_slice(&[1u8; 32]).unwrap();
        let sk2 = SecretKey::from_slice(&[2u8; 32]).unwrap();
        let pk2 = sk2.public_key(&secp);
        let pk1 = sk1.public_key(&secp);

        let plaintext = "Hello, NIP-44!";
        let payload = encrypt(&sk1, &pk2, plaintext).unwrap();
        let recovered = decrypt(&sk2, &pk1, &payload).unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn test_conversation_key_symmetry() {
        use secp256k1::Secp256k1;
        let secp = Secp256k1::new();
        let sk1 = SecretKey::from_slice(&[3u8; 32]).unwrap();
        let sk2 = SecretKey::from_slice(&[4u8; 32]).unwrap();
        let pk2 = sk2.public_key(&secp);
        let pk1 = sk1.public_key(&secp);

        let ck1 = ConversationKey::derive(&sk1, &pk2);
        let ck2 = ConversationKey::derive(&sk2, &pk1);
        assert_eq!(ck1, ck2, "conversation key must be symmetric");
    }
}
