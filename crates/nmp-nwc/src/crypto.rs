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

use crate::build::NwcBuildError;
use nostr::nips::{nip04, nip44};
use nostr::{Keys, PublicKey, SecretKey};

/// Encrypt `plaintext` from the NWC client to the wallet pubkey using NIP-04.
/// This is the historical NIP-47 default and the only flavor universally
/// supported across wallet implementations (Alby, Mutiny, Zeus, etc.).
///
/// # Errors
///
/// Returns `NwcBuildError` if the secret or pubkey are invalid or encryption fails.
pub fn encrypt(
    client_secret_hex: &str,
    wallet_pubkey_hex: &str,
    plaintext: &str,
) -> Result<String, NwcBuildError> {
    let sk = parse_secret(client_secret_hex)?;
    let pk = parse_pubkey(wallet_pubkey_hex)?;
    nip04::encrypt(&sk, &pk, plaintext).map_err(|e| NwcBuildError::Nip04Encrypt(e.to_string()))
}

/// Decrypt a kind:23195 response from the wallet pubkey to the NWC client.
///
/// Tries NIP-04 first (the historical NIP-47 default — payload contains `?iv=`).
/// Falls back to NIP-44 v2 for newer wallets that opt into it.
///
/// # Panic safety (D6)
///
/// The `?iv=` content here is attacker-controlled (it arrives over a relay).
/// `nostr`'s `nip04::decrypt` constructs an AES-CBC `GenericArray` from the
/// decoded IV via `iv.as_slice().into()`, which **panics** inside
/// `generic-array` if the IV is not exactly 16 bytes. A base64-valid but
/// wrong-length IV would therefore crash the wallet runtime. We pre-validate
/// the NIP-04 payload shape — `<base64-ciphertext>?iv=<base64-16-byte-iv>` —
/// and return `Err` for anything malformed before delegating.
///
/// # Errors
///
/// Returns `NwcBuildError` if the secret or pubkey are invalid, the payload is
/// malformed, or decryption fails.
pub fn decrypt(
    client_secret_hex: &str,
    wallet_pubkey_hex: &str,
    payload: &str,
) -> Result<String, NwcBuildError> {
    let sk = parse_secret(client_secret_hex)?;
    let pk = parse_pubkey(wallet_pubkey_hex)?;
    // NIP-04 payloads carry an `?iv=` query-string suffix; NIP-44 v2 payloads
    // are pure base64. Use the marker as a cheap discriminator.
    if payload.contains("?iv=") {
        validate_nip04_shape(payload)?;
        nip04::decrypt(&sk, &pk, payload).map_err(|e| NwcBuildError::Nip04Decrypt(e.to_string()))
    } else {
        nip44::decrypt(&sk, &pk, payload).map_err(|e| NwcBuildError::Nip44Decrypt(e.to_string()))
    }
}

/// Reject NIP-04 payloads that would panic `nip04::decrypt`.
///
/// A well-formed NIP-04 payload is exactly `<b64-ciphertext>?iv=<b64-iv>`
/// where the IV base64-decodes to exactly 16 bytes (the AES-CBC block size).
/// Anything else — extra `?iv=` markers, non-base64, or a wrong-length IV —
/// is rejected with `Err` so the caller sees a graceful failure.
fn validate_nip04_shape(payload: &str) -> Result<(), NwcBuildError> {
    let parts: Vec<&str> = payload.split("?iv=").collect();
    if parts.len() != 2 {
        return Err(NwcBuildError::MalformedNip04Payload(
            "malformed payload (expected one ?iv= marker)".to_string(),
        ));
    }
    // Ciphertext half must be valid base64 (length is checked downstream).
    base64_decode(parts[0]).ok_or_else(|| {
        NwcBuildError::MalformedNip04Payload("ciphertext is not valid base64".to_string())
    })?;
    // IV half must base64-decode to exactly one AES block (16 bytes).
    let iv = base64_decode(parts[1]).ok_or_else(|| {
        NwcBuildError::MalformedNip04Payload("iv is not valid base64".to_string())
    })?;
    if iv.len() != 16 {
        return Err(NwcBuildError::MalformedNip04Payload(format!(
            "iv must be 16 bytes, got {}",
            iv.len()
        )));
    }
    Ok(())
}

/// Minimal standard-alphabet base64 decoder (RFC 4648, `+/` with `=` padding).
///
/// Returns `None` for any input that is not valid base64. Used only to
/// length-check a NIP-04 IV before handing the payload to `nip04::decrypt`;
/// it is not on any hot path. The `nostr` crate does not re-export its
/// `base64` dependency, and adding a direct dep is out of scope here.
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    fn val(b: u8) -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes = s.as_bytes();
    // Strip up to two trailing '=' pad chars; padding must be the suffix only.
    let mut end = bytes.len();
    let mut pad = 0;
    while end > 0 && bytes[end - 1] == b'=' && pad < 2 {
        end -= 1;
        pad += 1;
    }
    let data = &bytes[..end];
    // With padding stripped, the symbol count mod 4 cannot be 1.
    if data.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(data.len() / 4 * 3 + 2);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for &b in data {
        let v = u32::from(val(b)?);
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            // Safe: after `bits -= 8`, acc >> bits always yields exactly 8 significant bits (≤ 255).
            #[allow(clippy::cast_possible_truncation)]
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// Derive the client public key from the client secret hex.
///
/// # Errors
///
/// Returns `NwcBuildError::InvalidClientSecret` if `client_secret_hex` is not a valid secp256k1 scalar.
pub fn client_pubkey_hex(client_secret_hex: &str) -> Result<String, NwcBuildError> {
    let sk = parse_secret(client_secret_hex)?;
    let keys = Keys::new(sk);
    Ok(keys.public_key().to_hex())
}

fn parse_secret(hex: &str) -> Result<SecretKey, NwcBuildError> {
    SecretKey::from_hex(hex).map_err(|e| NwcBuildError::InvalidClientSecret(e.to_string()))
}

fn parse_pubkey(hex: &str) -> Result<PublicKey, NwcBuildError> {
    PublicKey::from_hex(hex).map_err(|e| NwcBuildError::InvalidWalletPubkey(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Two deterministic, curve-valid secp256k1 scalars. `SecretKey::from_hex`
    // validates curve membership, so the `"a".repeat(64)`-style strings used in
    // the parser tests would not work here.
    const CLIENT_SECRET: &str =
        "0101010101010101010101010101010101010101010101010101010101010101";
    const WALLET_SECRET: &str =
        "0202020202020202020202020202020202020202020202020202020202020202";

    /// Encrypting then decrypting with the matching keypair yields the original
    /// plaintext. NIP-04 ciphertext is non-deterministic (random IV), so a
    /// round-trip — not a fixed-bytes assertion — is the only valid check.
    #[test]
    fn encrypt_decrypt_round_trip() {
        let wallet_pk = client_pubkey_hex(WALLET_SECRET).unwrap();
        let plaintext = r#"{"method":"get_balance","params":{}}"#;
        let ciphertext = encrypt(CLIENT_SECRET, &wallet_pk, plaintext).unwrap();
        assert_ne!(ciphertext, plaintext, "content must actually be encrypted");

        let client_pk = client_pubkey_hex(CLIENT_SECRET).unwrap();
        // Wallet decrypts with its own secret + the client's pubkey.
        let decrypted = decrypt(WALLET_SECRET, &client_pk, &ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    /// NIP-04 ECDH is symmetric: the client can decrypt a payload it itself
    /// encrypted to the wallet. This is exactly the build→decode test path.
    #[test]
    fn client_can_decrypt_own_outbound_payload() {
        let wallet_pk = client_pubkey_hex(WALLET_SECRET).unwrap();
        let plaintext = "round-trip me";
        let ciphertext = encrypt(CLIENT_SECRET, &wallet_pk, plaintext).unwrap();
        let decrypted = decrypt(CLIENT_SECRET, &wallet_pk, &ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    /// Decrypting with the wrong client secret must fail gracefully (Err, no
    /// panic) — D6. A leaked-amount bug here would be silent and severe.
    #[test]
    fn decrypt_with_wrong_secret_errs() {
        let wallet_pk = client_pubkey_hex(WALLET_SECRET).unwrap();
        let ciphertext = encrypt(CLIENT_SECRET, &wallet_pk, "secret data").unwrap();

        let wrong_secret =
            "0303030303030303030303030303030303030303030303030303030303030303";
        let result = decrypt(wrong_secret, &wallet_pk, &ciphertext);
        assert!(result.is_err(), "wrong key must not decrypt");
    }

    /// Garbage / truncated payloads must not panic the decryptor. The `?iv=`
    /// cases below are the regression net for a real D6 bug: `nip04::decrypt`
    /// panics inside `generic-array` when the IV base64-decodes to a length
    /// other than 16 bytes. `crypto::decrypt` pre-validates the shape so every
    /// one of these returns `Err` instead of crashing the wallet runtime.
    #[test]
    fn decrypt_malformed_payload_errs() {
        let wallet_pk = client_pubkey_hex(WALLET_SECRET).unwrap();
        let oversized_iv = format!("YWJj?iv={}", "A".repeat(1000));
        let two_markers = "YWJj?iv=YWJjYWJjYWJjYWJj?iv=YWJjYWJjYWJjYWJj";
        let cases: [&str; 12] = [
            "",
            "not-base64-at-all!!!",
            "AAAAAAAAAAAAAAAAAAAAAA", // NIP-44-shaped but too short / invalid
            "deadbeef?iv=ZZZZ",       // IV decodes to 3 bytes, not 16 — the panic case
            "?iv=",                   // empty ciphertext and empty IV
            "abc?iv=",                // empty IV
            "?iv=YWJjYWJjYWJjYWJj",   // empty ciphertext
            "abc?iv=YWJj",            // IV decodes to 3 bytes
            "!!!?iv=YWJjYWJjYWJjYWJj", // ciphertext not valid base64
            "YWJj?iv=!!!!!!!!!!!!!!!!", // IV not valid base64
            &oversized_iv,            // IV far longer than 16 bytes
            two_markers,              // two ?iv= markers
        ];
        for bad in cases {
            let result = decrypt(CLIENT_SECRET, &wallet_pk, bad);
            assert!(result.is_err(), "malformed payload {bad:?} must Err, not panic");
        }
    }

    /// `validate_nip04_shape` accepts a genuine `nip04::encrypt` output — the
    /// guard must not reject well-formed payloads.
    #[test]
    fn validate_nip04_shape_accepts_real_payload() {
        let wallet_pk = client_pubkey_hex(WALLET_SECRET).unwrap();
        let real = encrypt(CLIENT_SECRET, &wallet_pk, "hello").unwrap();
        assert!(real.contains("?iv="));
        assert!(validate_nip04_shape(&real).is_ok());
    }

    /// The inline base64 decoder must agree with a known RFC 4648 vector and
    /// reject obvious non-base64 input.
    #[test]
    fn base64_decode_basic_vectors() {
        assert_eq!(base64_decode("YWJj"), Some(b"abc".to_vec()));
        assert_eq!(base64_decode(""), Some(Vec::new()));
        assert_eq!(base64_decode("Zm9vYmFy"), Some(b"foobar".to_vec()));
        assert_eq!(base64_decode("Zg=="), Some(b"f".to_vec()));
        assert!(base64_decode("!!!!").is_none());
        assert!(base64_decode("YWJjY").is_none()); // length % 4 == 1
    }

    /// An invalid hex secret must surface an Err, never panic.
    #[test]
    fn encrypt_with_invalid_secret_errs() {
        let wallet_pk = client_pubkey_hex(WALLET_SECRET).unwrap();
        assert!(encrypt("not-hex", &wallet_pk, "x").is_err());
        assert!(encrypt(&"z".repeat(64), &wallet_pk, "x").is_err());
    }

    /// An invalid hex pubkey must surface an Err, never panic.
    #[test]
    fn encrypt_with_invalid_pubkey_errs() {
        assert!(encrypt(CLIENT_SECRET, "not-hex", "x").is_err());
        assert!(encrypt(CLIENT_SECRET, &"z".repeat(64), "x").is_err());
    }

    /// `client_pubkey_hex` must match the pubkey `nostr::Keys` derives for the
    /// same secret — this pubkey is what the wallet encrypts responses to.
    #[test]
    fn client_pubkey_hex_matches_keys_derivation() {
        let derived = client_pubkey_hex(CLIENT_SECRET).unwrap();
        let expected = Keys::new(SecretKey::from_hex(CLIENT_SECRET).unwrap())
            .public_key()
            .to_hex();
        assert_eq!(derived, expected);
        assert_eq!(derived.len(), 64, "x-only pubkey is 32 bytes hex");
    }

    /// An invalid secret passed to `client_pubkey_hex` must Err, not panic.
    #[test]
    fn client_pubkey_hex_invalid_secret_errs() {
        assert!(client_pubkey_hex("too-short").is_err());
        assert!(client_pubkey_hex(&"z".repeat(64)).is_err());
    }
}
