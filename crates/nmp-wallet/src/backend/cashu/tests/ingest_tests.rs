//! `ingest::ingest_wallet_config`/`ingest_token_event` (#2965) — the pure
//! decode/fold functions wallet recovery drives, exercised directly against a
//! bare `CashuWalletState` (no signer/decrypt port involved: these functions
//! take already-decrypted plaintext, see `ingest.rs`'s module docs).

use super::*;
use nmp_nip60::cashu::types::Proof;

/// A fresh secp256k1 keypair's (privkey hex, derived pubkey hex) — mirrors
/// `WalletConfig::generate`'s own key derivation so the expected pubkey in
/// assertions is computed the exact same way `ingest_wallet_config` computes
/// it, not re-derived independently.
fn fresh_cashu_keypair() -> (String, String) {
    let sk = nostr::secp256k1::SecretKey::new(&mut nostr::secp256k1::rand::thread_rng());
    let secp = nostr::secp256k1::Secp256k1::new();
    let pk = nostr::secp256k1::PublicKey::from_secret_key(&secp, &sk);
    (hex::encode(sk.secret_bytes()), hex::encode(pk.serialize()))
}

/// The exact `[[key, value], ...]` JSON shape `build_wallet_event`/
/// `wallet_config_plaintext` produce.
fn wallet_config_json(privkey_hex: &str, mints: &[&str]) -> String {
    let mut pairs: Vec<Vec<String>> = vec![vec!["privkey".to_string(), privkey_hex.to_string()]];
    for mint in mints {
        pairs.push(vec!["mint".to_string(), (*mint).to_string()]);
    }
    serde_json::to_string(&pairs).unwrap()
}

/// The exact `{mint, proofs, del}` JSON shape `build_token_event` produces.
fn token_event_json(mint: &str, proofs: &[Proof], del: &[&str]) -> String {
    serde_json::json!({
        "mint": mint,
        "proofs": proofs,
        "del": del,
    })
    .to_string()
}

// ─── ingest_wallet_config ───────────────────────────────────────────────────

#[test]
fn ingest_wallet_config_loads_privkey_and_mints() {
    let backend = CashuWalletBackend::new();
    let (privkey_hex, pubkey_hex) = fresh_cashu_keypair();
    let plaintext = wallet_config_json(&privkey_hex, &[MINT]);

    ingest::ingest_wallet_config(&backend.state, &plaintext).expect("ingest must succeed");

    let state = lock_state(&backend.state);
    assert!(state.created);
    assert_eq!(state.mints, vec![MINT.to_string()]);
    assert_eq!(state.cashu_pubkey_hex.as_deref(), Some(pubkey_hex.as_str()));
    assert!(state.cashu_privkey.is_some());
}

#[test]
fn ingest_wallet_config_never_clobbers_an_already_created_wallet() {
    let backend = CashuWalletBackend::new();
    {
        let mut state = lock_state(&backend.state);
        state.created = true;
        state.mints = vec!["https://original-mint.example".to_string()];
        state.cashu_pubkey_hex = Some("original-pubkey".to_string());
    }
    let (privkey_hex, _) = fresh_cashu_keypair();
    let plaintext = wallet_config_json(&privkey_hex, &["https://a-different-mint.example"]);

    ingest::ingest_wallet_config(&backend.state, &plaintext).expect("no-op ingest still Ok");

    let state = lock_state(&backend.state);
    assert_eq!(state.mints, vec!["https://original-mint.example".to_string()]);
    assert_eq!(state.cashu_pubkey_hex.as_deref(), Some("original-pubkey"));
}

#[test]
fn ingest_wallet_config_rejects_missing_privkey() {
    let backend = CashuWalletBackend::new();
    let plaintext = serde_json::to_string(&vec![vec!["mint", MINT]]).unwrap();
    let err = ingest::ingest_wallet_config(&backend.state, &plaintext)
        .expect_err("missing privkey must fail closed");
    assert!(err.contains("privkey"));
}

#[test]
fn ingest_wallet_config_rejects_empty_mints() {
    let backend = CashuWalletBackend::new();
    let (privkey_hex, _) = fresh_cashu_keypair();
    let plaintext = serde_json::to_string(&vec![vec!["privkey".to_string(), privkey_hex]]).unwrap();
    let err = ingest::ingest_wallet_config(&backend.state, &plaintext)
        .expect_err("no mints must fail closed");
    assert!(err.contains("mint"));
}

// ─── ingest_token_event ─────────────────────────────────────────────────────

#[test]
fn ingest_token_event_loads_proofs_into_state_and_ledger_balance() {
    let backend = backend_with_mint();
    let proof = synthetic_proof(21, "proof-c-1");
    let plaintext = token_event_json(MINT, std::slice::from_ref(&proof), &[]);

    ingest::ingest_token_event(&backend.state, "token-event-1", &plaintext, "wss://relay.example")
        .expect("ingest must succeed");

    let state = lock_state(&backend.state);
    assert_eq!(state.proofs.len(), 1);
    assert_eq!(state.proofs[0].proof.c, "proof-c-1");
    assert_eq!(
        state
            .ledger
            .state()
            .balance(
                &crate::journal::MintUrl::new(MINT),
                &crate::journal::WalletUnit::new("sat")
            ),
        21_000
    );
}

#[test]
fn ingest_token_event_is_idempotent_on_a_relay_resend() {
    let backend = backend_with_mint();
    let proof = synthetic_proof(10, "proof-c-resend");
    let plaintext = token_event_json(MINT, std::slice::from_ref(&proof), &[]);

    ingest::ingest_token_event(&backend.state, "token-event-resend", &plaintext, "").unwrap();
    ingest::ingest_token_event(&backend.state, "token-event-resend", &plaintext, "").unwrap();

    let state = lock_state(&backend.state);
    assert_eq!(state.proofs.len(), 1, "a re-observed token event must never double-count");
}

#[test]
fn ingest_token_event_del_field_supersedes_regardless_of_arrival_order() {
    let old_proof = synthetic_proof(5, "proof-old");
    let new_proof = synthetic_proof(8, "proof-new");
    let old_plaintext = token_event_json(MINT, std::slice::from_ref(&old_proof), &[]);
    let new_plaintext = token_event_json(MINT, std::slice::from_ref(&new_proof), &["token-old"]);

    // In-order: old observed, then the rollover that supersedes it.
    let in_order = backend_with_mint();
    ingest::ingest_token_event(&in_order.state, "token-old", &old_plaintext, "").unwrap();
    ingest::ingest_token_event(&in_order.state, "token-new", &new_plaintext, "").unwrap();
    {
        let state = lock_state(&in_order.state);
        assert_eq!(state.proofs.len(), 1);
        assert_eq!(state.proofs[0].proof.c, "proof-new");
    }

    // Out-of-order (cold-start replay has no ordering guarantee): the
    // superseding event observed FIRST must still win once the superseded
    // one is later (or never) observed.
    let out_of_order = backend_with_mint();
    ingest::ingest_token_event(&out_of_order.state, "token-new", &new_plaintext, "").unwrap();
    ingest::ingest_token_event(&out_of_order.state, "token-old", &old_plaintext, "").unwrap();
    {
        let state = lock_state(&out_of_order.state);
        assert_eq!(
            state.proofs.len(),
            1,
            "the superseded event's proofs must never load, however out of order"
        );
        assert_eq!(state.proofs[0].proof.c, "proof-new");
    }
}

#[test]
fn ingest_token_event_dedups_the_same_proof_c_across_unrelated_events() {
    let backend = backend_with_mint();
    let shared_proof = synthetic_proof(13, "shared-c");
    let first = token_event_json(MINT, std::slice::from_ref(&shared_proof), &[]);
    let second = token_event_json(MINT, std::slice::from_ref(&shared_proof), &[]);

    ingest::ingest_token_event(&backend.state, "token-a", &first, "").unwrap();
    ingest::ingest_token_event(&backend.state, "token-b", &second, "").unwrap();

    let state = lock_state(&backend.state);
    assert_eq!(
        state.proofs.len(),
        1,
        "the same proof C observed under two different (non-superseding) token \
         events must still be counted only once"
    );
}
