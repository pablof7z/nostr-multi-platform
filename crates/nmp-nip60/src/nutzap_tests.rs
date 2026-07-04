//! Split out of `nutzap.rs` (AGENTS.md file-size discipline).

use super::*;

#[test]
fn decode_nutzap_fields_matches_decode_nutzap_event() {
    let sender = Keys::generate();
    let recipient = Keys::generate();
    let proof = NutZapProof {
        amount: 10,
        id: "00deadbeef00".to_string(),
        secret: "s3cr3t".to_string(),
        c: "02".to_string() + &"ab".repeat(32),
        dleq: None,
    };
    let builder = build_nutzap_event(
        vec![proof],
        "https://mint.example",
        &recipient.public_key(),
        Some("gm"),
        None,
    )
    .expect("build nutzap");
    let event = builder.sign_with_keys(&sender).expect("sign nutzap");

    let via_event = decode_nutzap_event(&event).expect("decode via event");
    let via_fields = decode_nutzap_fields(
        &event.id.to_hex(),
        &event.pubkey.to_hex(),
        &event
            .tags
            .iter()
            .map(|t| t.as_slice().to_vec())
            .collect::<Vec<_>>(),
        &event.content,
    )
    .expect("decode via fields");

    assert_eq!(via_event.event_id, via_fields.event_id);
    assert_eq!(via_event.sender_pubkey, via_fields.sender_pubkey);
    assert_eq!(via_event.mint_url, via_fields.mint_url);
    assert_eq!(via_event.amount_sats, via_fields.amount_sats);
    assert_eq!(via_event.comment, via_fields.comment);
}

#[test]
fn decode_nutzap_info_fields_matches_decode_nutzap_info_event() {
    let keys = Keys::generate();
    let info = NutZapInfo {
        relays: vec!["wss://relay.example".to_string()],
        mints: vec!["https://mint.example".to_string()],
        cashu_pubkey: Some("02".to_string() + &"cd".repeat(32)),
    };
    let event = build_nutzap_info_event(&info, &keys)
        .expect("build info")
        .sign_with_keys(&keys)
        .expect("sign info");

    let via_event = decode_nutzap_info_event(&event);
    let via_fields = decode_nutzap_info_fields(
        &event
            .tags
            .iter()
            .map(|t| t.as_slice().to_vec())
            .collect::<Vec<_>>(),
    );

    assert_eq!(via_event.relays, via_fields.relays);
    assert_eq!(via_event.mints, via_fields.mints);
    assert_eq!(via_event.cashu_pubkey, via_fields.cashu_pubkey);
}

#[test]
fn p2pk_secret_pubkey_extracts_the_locked_key() {
    let pubkey = "02".to_string() + &"11".repeat(32);
    let secret = p2pk_secret(&pubkey);
    assert_eq!(p2pk_secret_pubkey(&secret), Some(pubkey));
}

#[test]
fn p2pk_secret_pubkey_rejects_a_non_p2pk_secret() {
    assert_eq!(p2pk_secret_pubkey("just-a-random-hex-secret"), None);
    assert_eq!(p2pk_secret_pubkey(r#"["HTLC", {}]"#), None);
}

#[test]
fn nutzap_info_tags_matches_build_nutzap_info_event() {
    let keys = Keys::generate();
    let info = NutZapInfo {
        relays: vec!["wss://relay.example".to_string()],
        mints: vec!["https://mint.example".to_string()],
        cashu_pubkey: Some("02".to_string() + &"ee".repeat(32)),
    };
    let event = build_nutzap_info_event(&info, &keys)
        .expect("build info")
        .sign_with_keys(&keys)
        .expect("sign info");
    let event_tags: Vec<Vec<String>> =
        event.tags.iter().map(|t| t.as_slice().to_vec()).collect();

    assert_eq!(nutzap_info_tags(&info), event_tags);
}

#[test]
fn nutzap_event_tags_matches_build_nutzap_event() {
    let sender = Keys::generate();
    let recipient = Keys::generate();
    let zapped = EventId::from_byte_array([3u8; 32]);
    let proof = NutZapProof {
        amount: 21,
        id: "00deadbeef00".to_string(),
        secret: "s3cr3t".to_string(),
        c: "02".to_string() + &"ab".repeat(32),
        dleq: None,
    };
    let proofs = vec![proof];
    let event = build_nutzap_event(
        proofs.clone(),
        "https://mint.example",
        &recipient.public_key(),
        Some("gm"),
        Some(&zapped),
    )
    .expect("build nutzap")
    .sign_with_keys(&sender)
    .expect("sign nutzap");
    let event_tags: Vec<Vec<String>> =
        event.tags.iter().map(|t| t.as_slice().to_vec()).collect();

    let tags = nutzap_event_tags(
        &proofs,
        "https://mint.example",
        &recipient.public_key(),
        Some(&zapped),
    )
    .expect("nutzap_event_tags");

    assert_eq!(tags, event_tags);
}

// ─── #2933 — missing DLEQ must fail closed, not pass silently ────────────

fn received_nutzap(proof: NutZapProof, mint_id: &str) -> ReceivedNutZap {
    ReceivedNutZap {
        event_id: EventId::from_byte_array([7u8; 32]),
        sender_pubkey: Keys::generate().public_key(),
        proofs: vec![proof],
        mint_url: format!("https://mint-{mint_id}.example"),
        amount_sats: 4,
        comment: String::new(),
        zapped_event_id: None,
    }
}

#[test]
fn verify_nutzap_dleq_rejects_a_proof_with_no_dleq() {
    let (keyset, _mint_sk) = crate::cashu::http::mint_http_support::fixture_keyset();
    let proof = NutZapProof {
        amount: 4,
        id: keyset.id.clone(),
        secret: "s3cr3t".to_string(),
        c: "02".to_string() + &"ab".repeat(32),
        dleq: None,
    };
    let nutzap = received_nutzap(proof, "no-dleq");

    let err = verify_nutzap_dleq_against_keyset(&nutzap, &keyset)
        .expect_err("a proof with no DLEQ must be rejected, never skipped");
    assert!(matches!(err, Nip60Error::Crypto(_)));
}

#[test]
fn verify_nutzap_dleq_rejects_a_dleq_missing_the_blinding_factor() {
    let (keyset, _mint_sk) = crate::cashu::http::mint_http_support::fixture_keyset();
    let proof = NutZapProof {
        amount: 4,
        id: keyset.id.clone(),
        secret: "s3cr3t".to_string(),
        c: "02".to_string() + &"ab".repeat(32),
        dleq: Some(crate::cashu::types::DleqProofWire {
            e: "00".repeat(32),
            s: "00".repeat(32),
            r: None,
        }),
    };
    let nutzap = received_nutzap(proof, "no-r");

    let err = verify_nutzap_dleq_against_keyset(&nutzap, &keyset)
        .expect_err("a DLEQ with no blinding factor r cannot be verified and must be rejected");
    assert!(matches!(err, Nip60Error::Crypto(_)));
}

/// The contrasting positive case: a genuinely valid DLEQ (built the same way
/// a real mint would sign one — see `mint_http_support::prove_dleq`) must
/// still pass, proving the #2933 fix only closes the missing-DLEQ hole, not
/// DLEQ verification itself.
#[test]
fn verify_nutzap_dleq_accepts_a_genuine_dleq() {
    use crate::cashu::crypto::{blind_message, unblind_signature};
    use crate::cashu::http::mint_http_support::{fixture_keyset, prove_dleq, secp};
    use nostr::secp256k1::{PublicKey as SecpPublicKey, Scalar, SecretKey};

    let (keyset, mint_sk) = fixture_keyset();
    let secp_ctx = secp();
    let secret = "nutzap-proof-secret".to_string();
    let r = SecretKey::new(&mut nostr::secp256k1::rand::thread_rng());
    let b_prime = blind_message(secret.as_bytes(), &r, &secp_ctx).expect("blind");
    let c_prime = b_prime
        .mul_tweak(&secp_ctx, &Scalar::from(mint_sk))
        .expect("mint sign C' = k*B'");
    let mint_pk = SecpPublicKey::from_secret_key(&secp_ctx, &mint_sk);
    let c = unblind_signature(&c_prime, &r, &mint_pk, &secp_ctx).expect("unblind");
    let (e_hex, s_hex) = prove_dleq(&b_prime, &c_prime, &mint_sk, &secp_ctx);

    let proof = NutZapProof {
        amount: 4,
        id: keyset.id.clone(),
        secret,
        c: hex::encode(c.serialize()),
        dleq: Some(crate::cashu::types::DleqProofWire {
            e: e_hex,
            s: s_hex,
            r: Some(hex::encode(r.secret_bytes())),
        }),
    };
    let nutzap = received_nutzap(proof, "valid");

    verify_nutzap_dleq_against_keyset(&nutzap, &keyset).expect("genuine DLEQ must verify");
}

// ─── #2963 — a proof's keyset id must bind to the keyset it's verified against ───

/// A proof whose DLEQ is genuinely valid against `keyset`'s mint key, but
/// whose `id` claims a *different* keyset, must still be rejected: `mint_pk`
/// is selected from `keyset` alone (by amount, not by `proof.id`), so without
/// an explicit `proof.id == keyset.id` gate a proof could claim any keyset id
/// and still verify against whatever keyset the caller happened to pass in.
/// This mirrors `blinded::finalize_blinded_outputs`'s `sig.id != keyset.id`
/// guard on the mint/swap path.
#[test]
fn verify_nutzap_dleq_rejects_a_proof_whose_keyset_id_does_not_match() {
    use crate::cashu::crypto::{blind_message, unblind_signature};
    use crate::cashu::http::mint_http_support::{fixture_keyset, prove_dleq, secp};
    use nostr::secp256k1::{PublicKey as SecpPublicKey, Scalar, SecretKey};

    let (keyset, mint_sk) = fixture_keyset();
    let secp_ctx = secp();
    let secret = "nutzap-proof-secret".to_string();
    let r = SecretKey::new(&mut nostr::secp256k1::rand::thread_rng());
    let b_prime = blind_message(secret.as_bytes(), &r, &secp_ctx).expect("blind");
    let c_prime = b_prime
        .mul_tweak(&secp_ctx, &Scalar::from(mint_sk))
        .expect("mint sign C' = k*B'");
    let mint_pk = SecpPublicKey::from_secret_key(&secp_ctx, &mint_sk);
    let c = unblind_signature(&c_prime, &r, &mint_pk, &secp_ctx).expect("unblind");
    let (e_hex, s_hex) = prove_dleq(&b_prime, &c_prime, &mint_sk, &secp_ctx);

    // A different, still-canonical (NUT-02 v1 "00" + 14 hex) keyset id — a
    // real rotated/other keyset, not a parse-error placeholder.
    let other_keyset_id = "00cafebeefdead00".to_string();
    assert_ne!(other_keyset_id, keyset.id);

    let proof = NutZapProof {
        amount: 4,
        id: other_keyset_id.clone(),
        secret,
        c: hex::encode(c.serialize()),
        dleq: Some(crate::cashu::types::DleqProofWire {
            e: e_hex,
            s: s_hex,
            r: Some(hex::encode(r.secret_bytes())),
        }),
    };
    let nutzap = received_nutzap(proof, "mismatched-keyset");

    let err = verify_nutzap_dleq_against_keyset(&nutzap, &keyset).expect_err(
        "a proof claiming a different keyset id than it's verified against must be rejected",
    );
    assert!(matches!(err, Nip60Error::Crypto(_)));
}
