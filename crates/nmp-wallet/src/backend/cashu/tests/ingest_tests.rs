//! `ingest::ingest_wallet_config`/`ingest_token_event` (#2965) — the pure
//! decode/fold functions wallet recovery drives, exercised directly against a
//! bare `CashuWalletState` (no signer/decrypt port involved: these functions
//! take already-decrypted plaintext, see `ingest.rs`'s module docs).

use super::*;
use nmp_nip60::cashu::types::{Proof, ProofSpendState};

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

/// #2972/#2973 — a recovered token event's mint must canonicalize the same
/// way `add_proofs`/`select_proofs` do, so a recovered proof's balance lands
/// under the same mint key a later `nutzap.send` resolves the recipient's
/// mint to, even when the two strings differ by scheme/host case or a
/// trailing slash (the real-sats minibits shape).
#[test]
fn ingest_token_event_canonicalizes_the_mint_url() {
    let backend = CashuWalletBackend::new();
    let proof = synthetic_proof(7, "proof-c-canon");
    let plaintext = token_event_json(
        "HTTPS://Mint.Minibits.Cash/Bitcoin/",
        std::slice::from_ref(&proof),
        &[],
    );

    ingest::ingest_token_event(&backend.state, "token-event-canon", &plaintext, "").unwrap();

    let state = lock_state(&backend.state);
    assert_eq!(
        state.proofs[0].mint, "https://mint.minibits.cash/Bitcoin",
        "the stored proof's mint must be canonical, matching what select_proofs compares against"
    );
    let (selected, total) = state
        .select_proofs("https://mint.minibits.cash/Bitcoin", 7)
        .expect("select_proofs must find the recovered proof by its canonical mint");
    assert_eq!(total, 7);
    assert_eq!(selected.len(), 1);
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

// ─── check-state on recovered proofs (#2977) ────────────────────────────────

/// A `Proof` with a distinct `secret`/`c` (unlike `synthetic_proof`'s fixed
/// placeholder) — the NUT-07 check-state pass keys verdicts on `c` and hashes
/// `secret`, so a test that must tell two recovered proofs apart needs both to
/// differ.
fn recoverable_proof(amount: u64, secret: &str, c: &str) -> Proof {
    Proof {
        amount,
        id: "keyset-1".to_string(),
        secret: secret.to_string(),
        c: c.to_string(),
        dleq: None,
        witness: None,
    }
}

fn sat_balance(backend: &CashuWalletBackend) -> u64 {
    lock_state(&backend.state).ledger.state().balance(
        &crate::journal::MintUrl::new(MINT),
        &crate::journal::WalletUnit::new("sat"),
    )
}

/// #2977 — the core money-relevant assertion: once the mint reports a
/// recovered proof `Spent`, folding that verdict drops it from BOTH the
/// spendable ledger balance AND the secret-bearing inventory, while an
/// `Unspent` sibling keeps counting. This is the exact `MintProbed{Spent}`
/// mechanism `send_worker.rs` folds post-swap, reused rather than reinvented.
#[test]
fn recovered_proof_reported_spent_is_dropped_from_spendable_balance() {
    let backend = backend_with_mint();
    let spent = recoverable_proof(21, "secret-spent", "c-spent");
    let unspent = recoverable_proof(8, "secret-unspent", "c-unspent");
    let plaintext = token_event_json(MINT, &[spent.clone(), unspent.clone()], &[]);

    ingest::ingest_token_event(&backend.state, "token-recovered", &plaintext, "")
        .expect("ingest must succeed");
    // Before reconciliation both proofs count (the transiently-optimistic
    // balance #2977 describes).
    assert_eq!(sat_balance(&backend), 29_000);

    ingest::fold_check_state_verdicts(
        &backend.state,
        &[
            ("c-spent".to_string(), ProofSpendState::Spent),
            ("c-unspent".to_string(), ProofSpendState::Unspent),
        ],
    );

    // The already-spent proof no longer counts; the unspent one still does.
    assert_eq!(
        sat_balance(&backend),
        8_000,
        "a recovered proof the mint reports spent must not count toward spendable balance"
    );
    let state = lock_state(&backend.state);
    let held: Vec<&str> = state.proofs.iter().map(|p| p.proof.c.as_str()).collect();
    assert_eq!(
        held,
        vec!["c-unspent"],
        "the spent proof must also leave the secret-bearing inventory so no send can select it"
    );
}

/// #2977 end-to-end over the real `MintClient::check_state` HTTP lane: recover
/// two proofs, point their mint at a mock that reports the first `SPENT` and
/// the second `UNSPENT`, and confirm `reconcile_recovered_proofs` leaves only
/// the unspent proof's balance. The mock's `Y` values are computed by the real
/// `build_check_state_request` so the response passes `parse_check_state_response`'s
/// ordering guard exactly as a live mint's reply would.
#[test]
fn reconcile_recovered_proofs_drops_mint_reported_spent_proof() {
    let spent = recoverable_proof(21, "secret-spent-e2e", "c-spent-e2e");
    let unspent = recoverable_proof(8, "secret-unspent-e2e", "c-unspent-e2e");
    // Compute the mock's `Y` values with the real request builder so the reply
    // passes `parse_check_state_response`'s ordering guard, then spawn the
    // one-response mock serving them.
    let (_, ys) = nmp_nip60::cashu::build_check_state_request(&[
        spent.secret.clone(),
        unspent.secret.clone(),
    ])
    .expect("build check-state request");
    let body = serde_json::json!({
        "states": [
            { "Y": ys[0], "state": "SPENT" },
            { "Y": ys[1], "state": "UNSPENT" },
        ]
    })
    .to_string();
    let mock_url = spawn_mock_mint(vec![(200, body)]);

    let backend = CashuWalletBackend::new();
    lock_state(&backend.state).mints = vec![mock_url.clone()];
    let plaintext = token_event_json(&mock_url, &[spent, unspent], &[]);
    let recovered = ingest::ingest_token_event(&backend.state, "token-e2e", &plaintext, "")
        .expect("ingest must succeed");
    assert_eq!(recovered.len(), 2, "both fresh proofs carried out for probing");

    ingest::reconcile_recovered_proofs(&backend.state, recovered);

    let state = lock_state(&backend.state);
    let canonical = crate::journal::MintUrl::new(mock_url.trim_end_matches('/'));
    assert_eq!(
        state
            .ledger
            .state()
            .balance(&canonical, &crate::journal::WalletUnit::new("sat")),
        8_000,
        "the mint-reported-spent proof must be reconciled out of spendable balance"
    );
    let held: Vec<&str> = state.proofs.iter().map(|p| p.proof.c.as_str()).collect();
    assert_eq!(held, vec!["c-unspent-e2e"]);
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
