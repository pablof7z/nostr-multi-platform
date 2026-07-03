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
