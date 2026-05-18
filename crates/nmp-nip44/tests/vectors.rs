//! NIP-44 v2 official test vector tests.
//!
//! Vectors sourced from:
//! <https://raw.githubusercontent.com/paulmillr/nip44/main/nip44.vectors.json>
//!
//! Covers:
//! - valid.get_conversation_key
//! - valid.get_message_keys
//! - valid.calc_padded_len
//! - valid.encrypt_decrypt
//! - valid.encrypt_decrypt_long_msg
//! - invalid.encrypt_msg_lengths
//! - invalid.get_conversation_key
//! - invalid.decrypt

use nmp_nip44::{
    calc_padded_len, decrypt_with_conversation_key, encrypt_with_conversation_key, ConversationKey,
};
use secp256k1::{PublicKey, Secp256k1, SecretKey};
use serde_json::Value;
use sha2::{Digest, Sha256};

fn hex_to_bytes32(hex: &str) -> [u8; 32] {
    let bytes = hex::decode(hex).expect("valid hex");
    bytes.try_into().expect("32 bytes")
}

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    hex::decode(hex).expect("valid hex")
}

fn load_vectors() -> Value {
    let json_str = include_str!("nip44.vectors.json");
    serde_json::from_str(json_str).expect("valid JSON")
}

// ── valid.get_conversation_key ────────────────────────────────────────────────

#[test]
fn test_valid_get_conversation_key() {
    let vectors = load_vectors();
    let cases = vectors["v2"]["valid"]["get_conversation_key"]
        .as_array()
        .expect("array");

    let secp = Secp256k1::new();

    for (i, case) in cases.iter().enumerate() {
        let sec1_hex = case["sec1"].as_str().unwrap();
        let pub2_hex = case["pub2"].as_str().unwrap();
        let expected_hex = case["conversation_key"].as_str().unwrap();

        let sk = SecretKey::from_slice(&hex_to_bytes32(sec1_hex))
            .expect("valid secret key");
        // pub2 is an x-only pubkey (32 bytes) — convert to compressed 33-byte format
        let pk = xonly_hex_to_pubkey(&secp, pub2_hex);
        let expected = hex_to_bytes32(expected_hex);

        let conv_key = ConversationKey::derive(&sk, &pk);
        assert_eq!(
            conv_key.as_bytes(),
            &expected,
            "get_conversation_key[{}] sec1={} pub2={}",
            i,
            sec1_hex,
            pub2_hex
        );
    }
}

// ── valid.get_message_keys ────────────────────────────────────────────────────

#[test]
fn test_valid_get_message_keys() {
    let vectors = load_vectors();
    let v2 = &vectors["v2"]["valid"]["get_message_keys"];
    let conv_key_hex = v2["conversation_key"].as_str().unwrap();
    let conv_key = ConversationKey::from_bytes(hex_to_bytes32(conv_key_hex));

    let keys = v2["keys"].as_array().expect("array");
    for (i, key_case) in keys.iter().enumerate() {
        let nonce = hex_to_bytes32(key_case["nonce"].as_str().unwrap());
        let expected_chacha_key = hex_to_bytes(key_case["chacha_key"].as_str().unwrap());
        let expected_chacha_nonce = hex_to_bytes(key_case["chacha_nonce"].as_str().unwrap());
        let expected_hmac_key = hex_to_bytes(key_case["hmac_key"].as_str().unwrap());

        let (chacha_key, chacha_nonce, hmac_key) = conv_key.message_keys(&nonce);

        assert_eq!(
            chacha_key.as_ref(),
            expected_chacha_key.as_slice(),
            "get_message_keys[{}] chacha_key mismatch",
            i
        );
        assert_eq!(
            chacha_nonce.as_ref(),
            expected_chacha_nonce.as_slice(),
            "get_message_keys[{}] chacha_nonce mismatch",
            i
        );
        assert_eq!(
            hmac_key.as_ref(),
            expected_hmac_key.as_slice(),
            "get_message_keys[{}] hmac_key mismatch",
            i
        );
    }
}

// ── valid.calc_padded_len ─────────────────────────────────────────────────────

#[test]
fn test_valid_calc_padded_len() {
    let vectors = load_vectors();
    let cases = vectors["v2"]["valid"]["calc_padded_len"]
        .as_array()
        .expect("array");

    for case in cases {
        let pair = case.as_array().expect("pair [unpadded, padded]");
        let unpadded = pair[0].as_u64().unwrap() as usize;
        let expected_padded = pair[1].as_u64().unwrap() as usize;
        let got = calc_padded_len(unpadded);
        assert_eq!(
            got, expected_padded,
            "calc_padded_len({}) = {} (expected {})",
            unpadded, got, expected_padded
        );
    }
}

// ── valid.encrypt_decrypt ─────────────────────────────────────────────────────

#[test]
fn test_valid_encrypt_decrypt() {
    let vectors = load_vectors();
    let cases = vectors["v2"]["valid"]["encrypt_decrypt"]
        .as_array()
        .expect("array");

    let secp = Secp256k1::new();

    for (i, case) in cases.iter().enumerate() {
        let conv_key_hex = case["conversation_key"].as_str().unwrap();
        let nonce_hex = case["nonce"].as_str().unwrap();
        let plaintext = case["plaintext"].as_str().unwrap();
        let expected_payload = case["payload"].as_str().unwrap();

        let conv_key = ConversationKey::from_bytes(hex_to_bytes32(conv_key_hex));
        let nonce = hex_to_bytes32(nonce_hex);

        // Test encryption produces expected payload
        let got_payload = encrypt_with_conversation_key(&conv_key, &nonce, plaintext)
            .expect("encrypt should succeed");
        assert_eq!(
            got_payload, expected_payload,
            "encrypt_decrypt[{}] payload mismatch for plaintext={:?}",
            i, plaintext
        );

        // Test decryption recovers original plaintext
        let got_plaintext = decrypt_with_conversation_key(&conv_key, &got_payload)
            .expect("decrypt should succeed");
        assert_eq!(
            got_plaintext, plaintext,
            "encrypt_decrypt[{}] decrypt mismatch for plaintext={:?}",
            i, plaintext
        );

        // Also verify we can also use sec1/sec2 keys for round-trip if present
        if let (Some(sec1_hex), Some(sec2_hex)) =
            (case["sec1"].as_str(), case["sec2"].as_str())
        {
            let sk1 = SecretKey::from_slice(&hex_to_bytes32(sec1_hex)).unwrap();
            let sk2 = SecretKey::from_slice(&hex_to_bytes32(sec2_hex)).unwrap();
            let pk2 = sk2.public_key(&secp);
            let pk1 = sk1.public_key(&secp);

            let derived_ck1 = ConversationKey::derive(&sk1, &pk2);
            let derived_ck2 = ConversationKey::derive(&sk2, &pk1);
            assert_eq!(
                derived_ck1, derived_ck2,
                "encrypt_decrypt[{}] conversation keys must be symmetric",
                i
            );
            assert_eq!(
                derived_ck1.as_bytes(),
                conv_key.as_bytes(),
                "encrypt_decrypt[{}] derived conv key matches vector",
                i
            );
        }
    }
}

// ── valid.encrypt_decrypt_long_msg ────────────────────────────────────────────

#[test]
fn test_valid_encrypt_decrypt_long_msg() {
    let vectors = load_vectors();
    let cases = vectors["v2"]["valid"]["encrypt_decrypt_long_msg"]
        .as_array()
        .expect("array");

    for (i, case) in cases.iter().enumerate() {
        let conv_key_hex = case["conversation_key"].as_str().unwrap();
        let nonce_hex = case["nonce"].as_str().unwrap();
        let pattern = case["pattern"].as_str().unwrap();
        let repeat = case["repeat"].as_u64().unwrap() as usize;
        let expected_plaintext_sha256 = case["plaintext_sha256"].as_str().unwrap();
        let expected_payload_sha256 = case["payload_sha256"].as_str().unwrap();

        let plaintext: String = pattern.repeat(repeat);

        // Verify plaintext hash
        let pt_hash = Sha256::digest(plaintext.as_bytes());
        let pt_hash_hex = hex::encode(pt_hash);
        assert_eq!(
            pt_hash_hex, expected_plaintext_sha256,
            "long_msg[{}] plaintext_sha256 mismatch",
            i
        );

        let conv_key = ConversationKey::from_bytes(hex_to_bytes32(conv_key_hex));
        let nonce = hex_to_bytes32(nonce_hex);

        let payload = encrypt_with_conversation_key(&conv_key, &nonce, &plaintext)
            .expect("long msg encrypt should succeed");

        // Verify payload hash
        let payload_hash = Sha256::digest(payload.as_bytes());
        let payload_hash_hex = hex::encode(payload_hash);
        assert_eq!(
            payload_hash_hex, expected_payload_sha256,
            "long_msg[{}] payload_sha256 mismatch",
            i
        );

        // Verify decrypt round-trip
        let recovered = decrypt_with_conversation_key(&conv_key, &payload)
            .expect("long msg decrypt should succeed");
        assert_eq!(recovered, plaintext, "long_msg[{}] roundtrip failed", i);
    }
}

// ── invalid.encrypt_msg_lengths ───────────────────────────────────────────────

#[test]
fn test_invalid_encrypt_msg_lengths() {
    let vectors = load_vectors();
    let cases = vectors["v2"]["invalid"]["encrypt_msg_lengths"]
        .as_array()
        .expect("array");

    let conv_key = ConversationKey::from_bytes([0u8; 32]);
    let nonce = [0u8; 32];

    for case in cases {
        let len = case.as_u64().unwrap() as usize;
        // Build a string of exactly `len` ASCII bytes (or 0 bytes for empty test)
        let plaintext = "x".repeat(len);
        let result = encrypt_with_conversation_key(&conv_key, &nonce, &plaintext);
        assert!(
            result.is_err(),
            "expected error for plaintext length={}, got Ok",
            len
        );
    }
}

// ── invalid.get_conversation_key ─────────────────────────────────────────────

#[test]
fn test_invalid_get_conversation_key() {
    let vectors = load_vectors();
    let cases = vectors["v2"]["invalid"]["get_conversation_key"]
        .as_array()
        .expect("array");

    let secp = Secp256k1::new();

    for (i, case) in cases.iter().enumerate() {
        let sec1_hex = case["sec1"].as_str().unwrap();
        let pub2_hex = case["pub2"].as_str().unwrap();
        let note = case["note"].as_str().unwrap_or("");

        // These should fail at key parsing (secp256k1 will reject invalid scalars)
        let sk_result = SecretKey::from_slice(&hex_to_bytes32(sec1_hex));
        let pk_result_opt = try_xonly_hex_to_pubkey(&secp, pub2_hex);

        // At least one of key parsing must fail, or ECDH must produce invalid output
        let both_parse = sk_result.is_ok() && pk_result_opt.is_some();
        if both_parse {
            // secp256k1 may still reject the secret key at ECDH time
            // For all cases in the spec, the secret key itself is invalid (0 or >= order)
            // so SecretKey::from_slice should have already failed above.
            panic!(
                "invalid_get_conversation_key[{}] note='{}': expected key parsing to fail but both sec1 and pub2 parsed OK",
                i, note
            );
        }
        // Good: key parsing rejected the invalid key
    }
}

// ── invalid.decrypt ───────────────────────────────────────────────────────────

#[test]
fn test_invalid_decrypt() {
    let vectors = load_vectors();
    let cases = vectors["v2"]["invalid"]["decrypt"]
        .as_array()
        .expect("array");

    for (i, case) in cases.iter().enumerate() {
        let conv_key_hex = case["conversation_key"].as_str().unwrap();
        let payload = case["payload"].as_str().unwrap();
        let note = case["note"].as_str().unwrap_or("");

        let conv_key = ConversationKey::from_bytes(hex_to_bytes32(conv_key_hex));
        let result = decrypt_with_conversation_key(&conv_key, payload);
        assert!(
            result.is_err(),
            "invalid_decrypt[{}] note='{}': expected error but got Ok(\"{}\")",
            i,
            note,
            result.unwrap()
        );
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Convert a 32-byte (64 hex char) x-only public key to a `secp256k1::PublicKey`.
/// NIP-44 vectors use x-only pubkeys; secp256k1 uses compressed (even-Y) form.
fn xonly_hex_to_pubkey(_secp: &Secp256k1<secp256k1::All>, hex_str: &str) -> PublicKey {
    let bytes = hex::decode(hex_str).expect("valid hex for pubkey");
    // Prepend 0x02 (even-Y compressed prefix) to make a 33-byte compressed pubkey
    let mut compressed = vec![0x02u8];
    compressed.extend_from_slice(&bytes);
    PublicKey::from_slice(&compressed).expect("valid compressed pubkey")
}

/// Try to convert a hex x-only pubkey, returning None on failure.
fn try_xonly_hex_to_pubkey(
    _secp: &Secp256k1<secp256k1::All>,
    hex_str: &str,
) -> Option<PublicKey> {
    let bytes = hex::decode(hex_str).ok()?;
    if bytes.len() != 32 {
        return None;
    }
    let mut compressed = vec![0x02u8];
    compressed.extend_from_slice(&bytes);
    PublicKey::from_slice(&compressed).ok()
}
